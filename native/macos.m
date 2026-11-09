// The platform boundary only: IOBluetooth callbacks and Unix socket bridging.
// Repository selection, Git framing and all application logic live in Rust.
#import <Foundation/Foundation.h>
#import <CoreFoundation/CoreFoundation.h>
#import <IOBluetooth/IOBluetooth.h>
#import <CoreBluetooth/CoreBluetooth.h>
#import <AppKit/AppKit.h>
#import <objc/runtime.h>
#include <dispatch/dispatch.h>
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <signal.h>
#include <spawn.h>
#include <sys/file.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

extern char **environ;
static NSString * const ServiceUUID = @"86b63824-1066-48f5-a694-fd5ba0f90ed5";
static NSMutableSet *LiveBridges;
static BOOL Guest = NO;
static BOOL ParentGone = NO;
static IOBluetoothSDPServiceRecord *PublishedRecord;
static IOBluetoothSDPUUID *PublishedToken;
static BluetoothRFCOMMChannelID PublishedChannel;
static IOBluetoothUserNotification *PublishedNotification;
static dispatch_source_t TerminationSource;

static BOOL RecordMatchesPublication(IOBluetoothSDPServiceRecord *record, IOBluetoothSDPUUID *token, int expected);

// IOBluetooth can return the requested channel even when another blugit
// process has already published it. Keep this lock in the native owner, not
// the state directory or Rust parent: listener shutdown may still be draining
// bridges, and a helper from another binary version shares the same adapter.
static int ChannelReservation = -1;

static void ReleaseReservation(void) {
    if (ChannelReservation >= 0) close(ChannelReservation);
    ChannelReservation = -1;
}

static const uint32_t AllChannelBits = UINT32_C(0x7ffffffe);

static int ReserveChannel(const char *directory, int requested, uint32_t used, int *selected) {
    if (requested < 0 || requested > 30 || (used & ~AllChannelBits)) { errno = EINVAL; return -1; }
    for (int channel = requested ? requested : 30; channel >= (requested ? requested : 1); --channel) {
        if (used & (UINT32_C(1) << channel)) continue;
        char path[PATH_MAX];
        int length = snprintf(path, sizeof(path), "%s/channel-%d.lock", directory, channel);
        if (length < 0 || (size_t)length >= sizeof(path)) { errno = ENAMETOOLONG; return -1; }
        int fd = open(path, O_CREAT | O_RDWR | O_CLOEXEC | O_NOFOLLOW, 0600);
        if (fd < 0) return -1;
        if (flock(fd, LOCK_EX | LOCK_NB) == 0) { *selected = channel; return fd; }
        int error = errno;
        close(fd);
        if (error != EWOULDBLOCK && error != EAGAIN) { errno = error; return -1; }
    }
    errno = EADDRINUSE;
    return -1;
}

#ifdef BLUGIT_TEST_TRANSPORT
int blugit_test_reserve_channel(const char *directory, int requested, int *selected) {
    return ReserveChannel(directory, requested, 0, selected);
}

int blugit_test_reserve_unused_channel(const char *directory, int requested, uint32_t used, int *selected) {
    return ReserveChannel(directory, requested, used, selected);
}
#endif

static BOOL RemoveOwnedPublication(IOBluetoothSDPServiceRecord *record, IOBluetoothSDPUUID *token, int expected) {
    if (!RecordMatchesPublication(record, token, expected)) return NO;
    BluetoothSDPServiceRecordHandle handle = 0;
    [record getServiceRecordHandle:&handle];
    IOReturn status = [record removeServiceRecord];
    if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS") || status != kIOReturnSuccess) {
        fprintf(stderr, "blugit Bluetooth: remove owned record=0x%x status=0x%x\n", handle, status);
    }
    return YES;
}

static void ReleaseService(void) {
    [PublishedNotification unregister]; PublishedNotification = nil;
    if (PublishedRecord) {
        if (!RemoveOwnedPublication(PublishedRecord, PublishedToken, PublishedChannel)) {
            fprintf(stderr, "blugit Bluetooth: ownership changed; leaving record untouched\n");
        }
        PublishedRecord = nil;
    }
    PublishedToken = nil;
    PublishedChannel = 0;
}

static void InstallTermination(void) {
    signal(SIGTERM, SIG_IGN);
    dispatch_queue_t queue = dispatch_get_global_queue(QOS_CLASS_UTILITY, 0);
    TerminationSource = dispatch_source_create(DISPATCH_SOURCE_TYPE_SIGNAL, SIGTERM, 0, queue);
    dispatch_source_set_event_handler(TerminationSource, ^{
        dispatch_async(dispatch_get_main_queue(), ^{
            if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS")) fprintf(stderr, "blugit Bluetooth: SIGTERM cleanup bridges=%lu\n", (unsigned long)LiveBridges.count);
            ReleaseService(); exit(0);
        });
        // A blocked OS connect must not leave an orphaned helper after Ctrl+C.
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 3 * NSEC_PER_SEC), queue, ^{ _exit(143); });
    });
    dispatch_resume(TerminationSource);
}

// dispatch_io_create can set O_NONBLOCK on its fd. dup() shares those file
// status flags with the reader, so EAGAIN is "wait for data", never EOF.
static ssize_t ReadWhenReady(int fd, void *buffer, size_t size) {
    for (;;) {
        ssize_t count = read(fd, buffer, size);
        if (count >= 0) return count;
        if (errno == EINTR) continue;
        if (errno != EAGAIN && errno != EWOULDBLOCK) return -1;
        struct pollfd wait = {.fd = fd, .events = POLLIN};
        int ready;
        do { ready = poll(&wait, 1, -1); } while (ready < 0 && errno == EINTR);
        if (ready < 0) return -1;
        if (wait.revents & POLLNVAL) { errno = EBADF; return -1; }
    }
}

#ifdef BLUGIT_TEST_TRANSPORT
// Exercise the real Dispatch I/O fd flag change without touching Bluetooth.
// The Rust regression test waits for this marker, then delays its first write.
ssize_t blugit_test_dispatch_read(int fd, void *buffer, size_t size) {
    int outputFD = dup(fd);
    if (outputFD < 0) return -1;
    dispatch_queue_t queue = dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0);
    dispatch_io_t channel = dispatch_io_create(DISPATCH_IO_STREAM, outputFD, queue, ^(int error) { (void)error; close(outputFD); });
    if (!channel) { close(outputFD); return -1; }
    dispatch_semaphore_t marker = dispatch_semaphore_create(0);
    __block int writeError = 0;
    dispatch_data_t bytes = dispatch_data_create("!", 1, NULL, DISPATCH_DATA_DESTRUCTOR_DEFAULT);
    dispatch_io_write(channel, 0, bytes, queue, ^(bool done, dispatch_data_t remaining, int error) {
        (void)remaining;
        if (error) writeError = error;
        if (done) dispatch_semaphore_signal(marker);
    });
    ssize_t total = 0;
    if (dispatch_semaphore_wait(marker, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC)) != 0 || writeError) {
        total = -1;
    } else if (!(fcntl(fd, F_GETFL) & O_NONBLOCK)) {
        // Do not silently pass if this OS did not exercise the actual bug.
        errno = EINVAL;
        total = -1;
    } else {
        while ((size_t)total < size) {
            ssize_t count = ReadWhenReady(fd, (char *)buffer + total, size - (size_t)total);
            if (count < 0) { total = -1; break; }
            if (count == 0) break;
            total += count;
        }
    }
    dispatch_io_close(channel, DISPATCH_IO_STOP);
    return total;
}
#endif

static void MonitorParent(void) {
    // stdout is a bidirectional private Unix socket, not a terminal. Losing the
    // Rust parent must not leave an orphaned Bluetooth service running.
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
        char ignored;
        for (;;) {
            ssize_t count = read(STDOUT_FILENO, &ignored, 1);
            if (count < 0 && errno == EINTR) continue;
            if (count <= 0) break;
        }
        dispatch_async(dispatch_get_main_queue(), ^{
            if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS")) fprintf(stderr, "blugit Bluetooth: parent control closed bridges=%lu\n", (unsigned long)LiveBridges.count);
            ParentGone = YES;
            ReleaseService();
            // Let active bridges drain queued final bytes/ACK before exiting.
            if (LiveBridges.count == 0) _exit(0);
        });
    });
}

int blugit_spawn(const char *executable, char *const *arguments, int output, pid_t *pid) {
    posix_spawnattr_t attributes;
    posix_spawn_file_actions_t actions;
    int error = posix_spawnattr_init(&attributes);
    if (error) return error;
    error = posix_spawn_file_actions_init(&actions);
    if (error) { posix_spawnattr_destroy(&attributes); return error; }
    // Apple responsibility attribution is applied by the spawning process. A
    // bundled child can then request Bluetooth permission under its own name.
    void (*disclaim)(posix_spawnattr_t *, int) = dlsym(RTLD_DEFAULT, "responsibility_spawnattrs_setdisclaim");
    if (disclaim) disclaim(&attributes, 1);
    // A terminal's Ctrl+C belongs to the Rust CLI. It decides when established
    // transfers have drained before terminating this Bluetooth owner.
    if (!error) error = posix_spawnattr_setpgroup(&attributes, 0);
    if (!error) error = posix_spawnattr_setflags(&attributes, POSIX_SPAWN_CLOEXEC_DEFAULT | POSIX_SPAWN_SETPGROUP);
    if (!error) error = posix_spawn_file_actions_adddup2(&actions, output, STDOUT_FILENO);
    if (!error) error = posix_spawn_file_actions_adddup2(&actions, STDERR_FILENO, STDERR_FILENO);
    if (!error) error = posix_spawn_file_actions_addopen(&actions, STDIN_FILENO, "/dev/null", O_RDONLY, 0);
    if (!error) error = posix_spawn(pid, executable, &actions, &attributes, arguments, environ);
    posix_spawn_file_actions_destroy(&actions);
    posix_spawnattr_destroy(&attributes);
    return error;
}

static void Fail(NSString *message) {
    fprintf(stderr, "blugit Bluetooth: %s\n", message.UTF8String);
    // stdout is the private control socket. Preserve the actual failure for
    // Rust instead of turning every helper exit into a permissions/radio error.
    NSString *oneLine = [[message stringByReplacingOccurrencesOfString:@"\n" withString:@" "]
        stringByReplacingOccurrencesOfString:@"\t" withString:@" "];
    printf("ERROR\t%s\n", oneLine.UTF8String);
    exit(1);
}

@interface PowerDelegate : NSObject <CBCentralManagerDelegate>
@end
@implementation PowerDelegate
- (void)centralManagerDidUpdateState:(CBCentralManager *)manager { (void)manager; }
@end

static const char *PowerStateName(CBManagerState state) {
    switch (state) {
        case CBManagerStateUnknown: return "unknown";
        case CBManagerStateResetting: return "resetting";
        case CBManagerStateUnsupported: return "unsupported";
        case CBManagerStateUnauthorized: return "unauthorized";
        case CBManagerStatePoweredOff: return "powered-off";
        case CBManagerStatePoweredOn: return "powered-on";
    }
    return "unrecognized";
}

static const char *AuthorizationName(CBManagerAuthorization authorization) {
    switch (authorization) {
        case CBManagerAuthorizationNotDetermined: return "not-determined";
        case CBManagerAuthorizationRestricted: return "restricted";
        case CBManagerAuthorizationDenied: return "denied";
        case CBManagerAuthorizationAllowedAlways: return "allowed";
    }
    return "unrecognized";
}

static CBCentralManager *WaitForBluetooth(void) {
    // This helper is started with posix_spawn, so complete AppKit registration
    // before TCC needs to present a first-use or updated-code permission prompt.
    NSApplication *application = NSApplication.sharedApplication;
    [application setActivationPolicy:NSApplicationActivationPolicyAccessory];
    [application finishLaunching];
    if (CBManager.authorization == CBManagerAuthorizationNotDetermined) {
        fprintf(stderr, "blugit Bluetooth: waiting for macOS permission approval\n");
        if (@available(macOS 14.0, *)) { [application activate]; }
        else { [NSRunningApplication.currentApplication activateWithOptions:0]; }
    }
    PowerDelegate *delegate = [PowerDelegate new];
    CBCentralManager *manager = [[CBCentralManager alloc] initWithDelegate:delegate queue:nil options:@{CBCentralManagerOptionShowPowerAlertKey: @NO}];
    if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS")) {
        fprintf(stderr, "blugit Bluetooth: state=%s authorization=%s\n", PowerStateName(manager.state), AuthorizationName(CBManager.authorization));
    }
    // Retain the delegate because CBCentralManager's delegate property is weak.
    static PowerDelegate *retained;
    retained = delegate;
    (void)retained;
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:55];
    while (manager.state == CBManagerStateUnknown || manager.state == CBManagerStateResetting || CBManager.authorization == CBManagerAuthorizationNotDetermined) {
        if (deadline.timeIntervalSinceNow <= 0) Fail([NSString stringWithFormat:@"Bluetooth initialization timed out (state=%s, authorization=%s); check blugit's Bluetooth permission prompt", PowerStateName(manager.state), AuthorizationName(CBManager.authorization)]);
        [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.05]];
    }
    if (manager.state != CBManagerStatePoweredOn) Fail([NSString stringWithFormat:@"Bluetooth is unavailable (state=%s, authorization=%s)", PowerStateName(manager.state), AuthorizationName(CBManager.authorization)]);
    return manager;
}

#ifdef BLUGIT_STANDALONE_HELPER
int blugit_macos_main(int argc, const char **argv);
int main(int argc, char **argv) {
    if (argc < 2 || strcmp(argv[1], "__bluetooth") != 0) return 2;
    return blugit_macos_main(argc - 2, (const char **)(argv + 2));
}
#endif

@interface Bridge : NSObject <IOBluetoothRFCOMMChannelDelegate>
@property(nonatomic, strong) IOBluetoothRFCOMMChannel *channel;
@property(nonatomic, strong) dispatch_io_t output;
@property(nonatomic, strong) dispatch_semaphore_t written;
@property(nonatomic, strong) NSData *writing;
@property(nonatomic, strong) NSMutableData *early;
@property(nonatomic) int fd;
@property(atomic) BOOL closed;
@property(nonatomic) size_t mtu;
@property(nonatomic) NSUInteger pending;
@property(nonatomic) BOOL loggedLocalRead;
@property(nonatomic) BOOL loggedRFCOMMData;
- (BOOL)attach:(IOBluetoothRFCOMMChannel *)channel path:(NSString *)path;
- (void)finish;
@end

@implementation Bridge
- (instancetype)init {
    if ((self = [super init])) { _fd = -1; _early = [NSMutableData new]; }
    return self;
}
- (BOOL)attach:(IOBluetoothRFCOMMChannel *)channel path:(NSString *)path {
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) { fprintf(stderr, "blugit Bluetooth: local socket failed: %s\n", strerror(errno)); return NO; }
    struct sockaddr_un address = {0};
    address.sun_family = AF_UNIX;
    const char *name = path.fileSystemRepresentation;
    if (strlen(name) >= sizeof(address.sun_path)) { fprintf(stderr, "blugit Bluetooth: local socket path is too long\n"); close(fd); return NO; }
    strlcpy(address.sun_path, name, sizeof(address.sun_path));
    if (connect(fd, (struct sockaddr *)&address, sizeof(address)) != 0) {
        fprintf(stderr, "blugit Bluetooth: local connect failed: %s\n", strerror(errno)); close(fd); return NO;
    }
    if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS")) {
        fprintf(stderr, "blugit Bluetooth: local stream attached bridge=%p channel=%u object=%llu fd=%d pending=%lu\n", (__bridge void *)self, channel.getChannelID, (unsigned long long)channel.getObjectID, fd, (unsigned long)self.pending);
    }
    int yes = 1;
    setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &yes, sizeof(yes));
    self.fd = fd;
    self.channel = channel;
    self.mtu = MAX(1, MIN(channel.getMTU, 65535));
    self.written = dispatch_semaphore_create(0);
    int outputFD = dup(fd);
    if (outputFD < 0) { close(fd); self.fd = -1; return NO; }
    self.output = dispatch_io_create(DISPATCH_IO_STREAM, outputFD, dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^(int error) { (void)error; close(outputFD); });
    IOReturn delegateStatus = [channel setDelegate:self];
    if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS")) fprintf(stderr, "blugit Bluetooth: bridge=%p delegate status=0x%x channel=%u object=%llu\n", (__bridge void *)self, delegateStatus, channel.getChannelID, (unsigned long long)channel.getObjectID);
    [LiveBridges addObject:self];
    if (self.early.length) {
        NSData *early = [self.early copy];
        self.early = nil;
        [self rfcommChannelData:channel data:(void *)early.bytes length:early.length];
    }
    __weak Bridge *weak = self;
    // Only one MTU-sized RFCOMM write is in flight. The reader thread waits for
    // its completion, keeping backpressure off the main delegate/run-loop thread.
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        for (;;) {
            @autoreleasepool {
            Bridge *bridge = weak;
            if (!bridge || bridge.closed) break;
            size_t mtu = bridge.mtu;
            NSMutableData *buffer = [NSMutableData dataWithLength:mtu];
            ssize_t count = ReadWhenReady(fd, buffer.mutableBytes, mtu);
            int readError = count < 0 ? errno : 0;
            if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS") && !bridge.loggedLocalRead) {
                bridge.loggedLocalRead = YES;
                dispatch_async(dispatch_get_main_queue(), ^{
                    fprintf(stderr, "blugit Bluetooth: bridge=%p first local read channel=%u object=%llu length=%zd pending=%lu closed=%d\n", (__bridge void *)bridge, bridge.channel.getChannelID, (unsigned long long)bridge.channel.getObjectID, count, (unsigned long)bridge.pending, bridge.closed);
                });
            }
            if (count <= 0) {
                int error = readError;
                dispatch_async(dispatch_get_main_queue(), ^{
                    if (error) fprintf(stderr, "blugit Bluetooth: local stream read failed: %s\n", strerror(error));
                    [weak finish];
                });
                break;
            }
            buffer.length = (NSUInteger)count;
            dispatch_async(dispatch_get_main_queue(), ^{
                Bridge *current = weak;
                if (!current || current.closed) { dispatch_semaphore_signal(bridge.written); return; }
                current.writing = buffer;
                IOReturn status = [current.channel writeAsync:(void *)buffer.bytes length:(UInt16)buffer.length refcon:NULL];
                if (status != kIOReturnSuccess) { [current finish]; }
            });
            if (dispatch_semaphore_wait(bridge.written, dispatch_time(DISPATCH_TIME_NOW, 30 * NSEC_PER_SEC)) != 0) {
                dispatch_async(dispatch_get_main_queue(), ^{ [weak finish]; });
                break;
            }
            }
        }
        // finish() shuts down the socket to wake this reader. Keep the fd alive
        // until the reader is finished, then close it on the delegate queue so
        // another connection cannot reuse its number before finish() runs.
        dispatch_async(dispatch_get_main_queue(), ^{
            Bridge *bridge = weak;
            [bridge finish];
            close(fd);
            if (bridge.fd == fd) bridge.fd = -1;
        });
    });
    return YES;
}
- (void)rfcommChannelWriteComplete:(IOBluetoothRFCOMMChannel *)channel refcon:(void *)refcon status:(IOReturn)status bytesWritten:(size_t)length {
    (void)channel; (void)refcon; (void)length;
    if (!self.writing) return;
    self.writing = nil;
    dispatch_semaphore_signal(self.written);
    if (status != kIOReturnSuccess) [self finish];
}
- (void)rfcommChannelWriteComplete:(IOBluetoothRFCOMMChannel *)channel refcon:(void *)refcon status:(IOReturn)status {
    [self rfcommChannelWriteComplete:channel refcon:refcon status:status bytesWritten:self.writing.length];
}
- (void)rfcommChannelData:(IOBluetoothRFCOMMChannel *)channel data:(void *)data length:(size_t)length {
    if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS") && !self.loggedRFCOMMData) {
        self.loggedRFCOMMData = YES;
        fprintf(stderr, "blugit Bluetooth: bridge=%p first RFCOMM data channel=%u object=%llu length=%zu pending=%lu closed=%d\n", (__bridge void *)self, channel.getChannelID, (unsigned long long)channel.getObjectID, length, (unsigned long)self.pending, self.closed);
    }
    if (self.closed) return;
    if (!self.output) {
        if (self.early.length + length > 8 * 1024 * 1024) { [self finish]; return; }
        [self.early appendBytes:data length:length];
        return;
    }
    // Copy bytes before the framework reuses its callback buffer. dispatch_io
    // drains them without ever blocking IOBluetooth's delegate thread.
    self.pending += length;
    if (self.pending > 8 * 1024 * 1024) { [self finish]; return; }
    void *copy = malloc(length);
    if (!copy) { [self finish]; return; }
    memcpy(copy, data, length);
    dispatch_data_t bytes = dispatch_data_create(copy, length, NULL, DISPATCH_DATA_DESTRUCTOR_FREE);
    __weak Bridge *weak = self;
    dispatch_io_write(self.output, 0, bytes, dispatch_get_main_queue(), ^(bool done, dispatch_data_t remaining, int error) {
        (void)remaining;
        Bridge *bridge = weak;
        if (done) bridge.pending -= MIN(bridge.pending, length);
        if (error) [bridge finish];
    });
}
- (void)rfcommChannelClosed:(IOBluetoothRFCOMMChannel *)channel {
    if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS")) fprintf(stderr, "blugit Bluetooth: bridge=%p closed channel=%u object=%llu pending=%lu already-closed=%d\n", (__bridge void *)self, channel.getChannelID, (unsigned long long)channel.getObjectID, (unsigned long)self.pending, self.closed);
    if (self.output) {
        __weak Bridge *weak = self;
        dispatch_io_barrier(self.output, ^{ dispatch_async(dispatch_get_main_queue(), ^{ [weak finish]; }); });
    } else { [self finish]; }
}
- (void)finish {
    if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS")) fprintf(stderr, "blugit Bluetooth: bridge=%p finish channel=%u object=%llu pending=%lu already-closed=%d\n", (__bridge void *)self, self.channel.getChannelID, (unsigned long long)self.channel.getObjectID, (unsigned long)self.pending, self.closed);
    if (self.closed) return;
    self.closed = YES;
    [self.channel setDelegate:nil];
    [self.channel closeChannel];
    if (self.fd >= 0) shutdown(self.fd, SHUT_RDWR);
    if (self.output) dispatch_io_close(self.output, DISPATCH_IO_STOP);
    if (self.written) dispatch_semaphore_signal(self.written);
    [LiveBridges removeObject:self];
    if (Guest || (ParentGone && LiveBridges.count == 0)) exit(0);
}
@end

static BOOL AcceptNotification(BOOL registrationComplete, BOOL own, BOOL existing, int expected, int actual, BOOL incoming) {
    return registrationComplete && own && !existing && expected >= 1 && expected <= 30 && actual == expected && incoming;
}

@interface Listener : NSObject
@property(nonatomic, strong) NSString *path;
@property(nonatomic) BluetoothRFCOMMChannelID expected;
@property(nonatomic) BOOL registrationComplete;
@property(nonatomic, strong) NSMutableSet<NSString *> *existingChannels;
@end
@implementation Listener
- (void)opened:(IOBluetoothUserNotification *)notification channel:(IOBluetoothRFCOMMChannel *)channel {
    // Registration can synchronously report already-open channels. Remember
    // them and never attach, close, or change their delegates, even if repeated.
    NSString *object = [NSString stringWithFormat:@"%u:%llu", channel.getChannelID, (unsigned long long)channel.getObjectID];
    if (!self.registrationComplete) [self.existingChannels addObject:object];
    BOOL existing = [self.existingChannels containsObject:object];
    for (Bridge *bridge in LiveBridges) {
        if (bridge.channel == channel || bridge.channel.getObjectID == channel.getObjectID) existing = YES;
    }
    BOOL own = PublishedNotification && notification == PublishedNotification
        && RecordMatchesPublication(PublishedRecord, PublishedToken, self.expected);
    if (!AcceptNotification(self.registrationComplete, own, existing, self.expected, channel.getChannelID, channel.isIncoming)) {
        if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS")) {
            fprintf(stderr, "blugit Bluetooth: ignored notification own=%d channel=%u incoming=%d expected=%u existing=%d registering=%d\n", own, channel.getChannelID, channel.isIncoming, self.expected, existing, !self.registrationComplete);
        }
        return;
    }
    if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS")) {
        fprintf(stderr, "blugit Bluetooth: incoming channel=%u incoming=%d open=%d object=%llu expected=%u\n", channel.getChannelID, channel.isIncoming, channel.isOpen, (unsigned long long)channel.getObjectID, self.expected);
    }
    // macOS cannot safely hold two RFCOMM channels to the same device.
    for (Bridge *bridge in LiveBridges) {
        if ([bridge.channel.getDevice.addressString isEqualToString:channel.getDevice.addressString]) {
            if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS")) fprintf(stderr, "blugit Bluetooth: rejected duplicate peer bridge\n");
            return;
        }
    }
    Bridge *bridge = [Bridge new];
    if (![bridge attach:channel path:self.path]) [channel closeChannel];
}
@end

static NSDictionary *SDPUnsigned32(uint32_t value) {
    return @{@"DataElementType": @1, @"DataElementSize": @4, @"DataElementValue": @(value)};
}

static NSArray *PublicationGenerationValue(uint64_t value) {
    // The macOS SDP daemon swaps the 32-bit halves of uint64 elements, even
    // when supplied as network-order NSData. Two uint32 elements survive the
    // real SDK publication path without that conversion.
    return @[SDPUnsigned32((uint32_t)(value >> 32)), SDPUnsigned32((uint32_t)value)];
}

static uint64_t ReadPublicationGeneration(IOBluetoothSDPDataElement *element) {
    if (element.getTypeDescriptor != 6) return 0;
    NSArray *parts = element.getArrayValue;
    if (parts.count != 2) return 0;
    uint64_t values[2];
    for (NSUInteger index = 0; index < 2; ++index) {
        IOBluetoothSDPDataElement *part = parts[index];
        if (![part isKindOfClass:IOBluetoothSDPDataElement.class] || part.getTypeDescriptor != 1 || part.getSize != 4) return 0;
        NSNumber *number = part.getNumberValue;
        if (!number || number.unsignedLongLongValue > UINT32_MAX) return 0;
        values[index] = number.unsignedLongLongValue;
    }
    return (values[0] << 32) | values[1];
}

static IOBluetoothSDPUUID *UUIDValue(NSString *text) {
    uuid_t bytes;
    NSUUID *uuid = [[NSUUID alloc] initWithUUIDString:text];
    if (!uuid) return nil;
    [uuid getUUIDBytes:bytes];
    return [IOBluetoothSDPUUID uuidWithBytes:bytes length:16];
}

static NSArray *SDPSequence(id element) {
    if (![element isKindOfClass:IOBluetoothSDPDataElement.class] || [element getTypeDescriptor] != 6) return nil;
    id parts = [element getArrayValue];
    return [parts isKindOfClass:NSArray.class] ? parts : nil;
}

static IOBluetoothSDPUUID *SDPUUIDValue(id element) {
    if (![element isKindOfClass:IOBluetoothSDPDataElement.class] || [element getTypeDescriptor] != 3) return nil;
    uint32_t size = [element getSize];
    if (size != 2 && size != 4 && size != 16) return nil;
    id uuid = [element getUUIDValue];
    return [uuid isKindOfClass:IOBluetoothSDPUUID.class] && [uuid length] == size ? uuid : nil;
}

static BOOL SDPMatchesUUID(id element, IOBluetoothSDPUUID *expected) {
    IOBluetoothSDPUUID *uuid = SDPUUIDValue(element);
    return uuid && expected && [uuid isEqualToUUID:expected];
}

typedef NS_ENUM(int, RFCOMMDescriptorStatus) { RFCOMMAbsent, RFCOMMValid, RFCOMMMalformed, RFCOMMInactive };

static RFCOMMDescriptorStatus ParseRFCOMMChannel(IOBluetoothSDPDataElement *element, BluetoothRFCOMMChannelID *channel) {
    // Do not use getRFCOMMChannelID: the local SDK getter can disagree with
    // its ProtocolDescriptorList. Accept uint8 and SDK-normalized uint16/32.
    @try {
        if (!element) return RFCOMMAbsent;
        NSArray *protocols = SDPSequence(element);
        if (!protocols || protocols.count == 0 || protocols.count > 16) return RFCOMMMalformed;
        BOOL found = NO;
        BOOL inactive = NO;
        BluetoothRFCOMMChannelID selected = 0;
        for (id descriptor in protocols) {
            NSArray *parts = SDPSequence(descriptor);
            if (!parts || parts.count == 0 || parts.count > 16) return RFCOMMMalformed;
            IOBluetoothSDPUUID *uuid = SDPUUIDValue(parts[0]);
            if (!uuid) return RFCOMMMalformed;
            if (![uuid isEqualToUUID:[IOBluetoothSDPUUID uuid16:0x0003]]) continue;
            if (found || (parts.count != 1 && parts.count != 2)) return RFCOMMMalformed;
            found = YES;
            // Inventory only: these exact, well-typed forms advertise no
            // usable number. Adoption/discovery still reject them below.
            if (parts.count == 1) { inactive = YES; continue; }
            id numberElement = parts[1];
            if (![numberElement isKindOfClass:IOBluetoothSDPDataElement.class] || [numberElement getTypeDescriptor] != 1) return RFCOMMMalformed;
            uint32_t size = [numberElement getSize];
            if (size != 1 && size != 2 && size != 4) return RFCOMMMalformed;
            id number = [numberElement getNumberValue];
            if (![number isKindOfClass:NSNumber.class] || CFGetTypeID((__bridge CFTypeRef)number) == CFBooleanGetTypeID()) return RFCOMMMalformed;
            const char *type = [number objCType];
            if (!type || !strchr("cCsSiIlLqQ", type[0]) || type[1]) return RFCOMMMalformed;
            uint64_t value = [number unsignedLongLongValue];
            if (value > 30) return RFCOMMMalformed;
            selected = (BluetoothRFCOMMChannelID)value;
            inactive = value == 0;
        }
        if (inactive) return RFCOMMInactive;
        if (found) *channel = selected;
        return found ? RFCOMMValid : RFCOMMAbsent;
    } @catch (NSException *exception) {
        (void)exception;
        return RFCOMMMalformed;
    }
}

static BOOL ReadRFCOMMChannel(IOBluetoothSDPDataElement *element, BluetoothRFCOMMChannelID *channel) {
    return ParseRFCOMMChannel(element, channel) == RFCOMMValid;
}

typedef NS_ENUM(int, LocalServicesError) {
    LocalServicesOK, LocalServicesMissingClass, LocalServicesMissingSelector,
    LocalServicesBadSignature, LocalServicesBadManager, LocalServicesBadCollection,
    LocalServicesTooMany, LocalServicesBadRecord, LocalServicesBadProtocol,
    LocalServicesException
};

static NSString *LocalServicesErrorMessage(LocalServicesError error) {
    switch (error) {
        case LocalServicesMissingClass: return @"local service manager is unavailable";
        case LocalServicesMissingSelector: return @"local service query is unavailable";
        case LocalServicesBadSignature: return @"local service query has an unsupported signature";
        case LocalServicesBadManager: return @"local service query returned an invalid manager";
        case LocalServicesBadCollection: return @"local service query did not return a dictionary";
        case LocalServicesTooMany: return @"local service query exceeded 1024 records";
        case LocalServicesBadRecord: return @"local service query returned an invalid or remote record";
        case LocalServicesBadProtocol: return @"a local service has an invalid RFCOMM descriptor";
        case LocalServicesException: return @"local service query raised an exception";
        default: return @"unknown local service query result";
    }
}

// Only the two fixed, read-only getters below use this invocation boundary.
// Check the runtime ABI before sending an undeclared private selector. No
// private publication, removal, power, or connection operation is permitted.
static BOOL ValidLocalGetterSignature(NSMethodSignature *signature) {
    const char *type = signature.methodReturnType;
    if (signature.numberOfArguments != 2 || signature.methodReturnLength != sizeof(id)
        || !type || type[0] != '@' || (type[1] && type[1] != '"')
        || (strcmp([signature getArgumentTypeAtIndex:0], "@") && strcmp([signature getArgumentTypeAtIndex:0], "#"))
        || strcmp([signature getArgumentTypeAtIndex:1], ":")) return NO;
    return YES;
}

static LocalServicesError InvokeLocalServicesGetter(id target, SEL selector, id *result) {
    Method method = object_isClass(target)
        ? class_getClassMethod((Class)target, selector)
        : class_getInstanceMethod(object_getClass(target), selector);
    if (!method) return LocalServicesMissingSelector;
    NSMethodSignature *signature = [NSMethodSignature signatureWithObjCTypes:method_getTypeEncoding(method)];
    if (!ValidLocalGetterSignature(signature)) return LocalServicesBadSignature;
    NSInvocation *invocation = [NSInvocation invocationWithMethodSignature:signature];
    invocation.selector = selector;
    [invocation invokeWithTarget:target];
    __unsafe_unretained id value = nil;
    [invocation getReturnValue:&value];
    *result = value;
    return LocalServicesOK;
}

static LocalServicesError ChannelsFromLocalServices(id services, uint32_t *used) {
    if (![services isKindOfClass:NSDictionary.class]) return LocalServicesBadCollection;
    if ([services count] > 1024) return LocalServicesTooMany;
    NSDictionary *snapshot = [services copy];
    if (![snapshot isKindOfClass:NSDictionary.class]) return LocalServicesBadCollection;
    if (snapshot.count > 1024) return LocalServicesTooMany;
    id records = snapshot.allValues;
    if (![records isKindOfClass:NSArray.class] || [records count] != snapshot.count) return LocalServicesBadCollection;
    uint32_t found = 0;
    for (id object in records) {
        if (![object isKindOfClass:IOBluetoothSDPServiceRecord.class] || [object device]) return LocalServicesBadRecord;
        BluetoothRFCOMMChannelID channel = 0;
        RFCOMMDescriptorStatus status = ParseRFCOMMChannel([object getAttributeDataElement:0x0004], &channel);
        if (status == RFCOMMMalformed) return LocalServicesBadProtocol;
        if (status == RFCOMMValid) found |= UINT32_C(1) << channel;
    }
    // On any failure, leave the caller's snapshot untouched. Never mistake
    // an unreadable or partial inventory for an empty set of advertisements.
    *used = found;
    return LocalServicesOK;
}

static LocalServicesError UsedChannelsFromManager(Class managerClass, uint32_t *used) {
    @try {
        if (!managerClass) return LocalServicesMissingClass;
        id manager = nil;
        LocalServicesError error = InvokeLocalServicesGetter(managerClass, NSSelectorFromString(@"sharedDeviceManager"), &manager);
        if (error != LocalServicesOK) return error;
        if (![manager isKindOfClass:managerClass]) return LocalServicesBadManager;
        id services = nil;
        error = InvokeLocalServicesGetter(manager, NSSelectorFromString(@"getLocalServices"), &services);
        if (error != LocalServicesOK) return error;
        return ChannelsFromLocalServices(services, used);
    } @catch (NSException *exception) {
        (void)exception;
        return LocalServicesException;
    }
}

static uint32_t UsedLocalChannels(void) {
    // macOS keeps retired advertisements. Read all classes, including our own,
    // without changing them, rather than trusting publication success as a
    // reachability check. This optional-at-runtime SPI must fail closed.
    uint32_t used = 0;
    LocalServicesError error = UsedChannelsFromManager(NSClassFromString(@"BluetoothDeviceManager"), &used);
    if (error != LocalServicesOK) {
        Fail([NSString stringWithFormat:@"Cannot safely choose an unused RFCOMM channel: %@; no service was published", LocalServicesErrorMessage(error)]);
    }
    return used;
}

static BOOL RecordMatchesPublication(IOBluetoothSDPServiceRecord *record, IOBluetoothSDPUUID *token, int expected) {
    @try {
        if (![record isKindOfClass:IOBluetoothSDPServiceRecord.class] || !token || expected < 1 || expected > 30) return NO;
        NSArray *classes = SDPSequence([record getAttributeDataElement:0x0001]);
        if (classes.count != 1 || !SDPMatchesUUID(classes[0], UUIDValue(ServiceUUID))) return NO;
        if (!SDPMatchesUUID([record getAttributeDataElement:0x0201], token)) return NO;
        BluetoothRFCOMMChannelID channel = 0;
        return ReadRFCOMMChannel([record getAttributeDataElement:0x0004], &channel) && channel == expected;
    } @catch (NSException *exception) {
        (void)exception;
        return NO;
    }
}

static NSDictionary *ServiceRecord(int channel, IOBluetoothSDPUUID *token) {
    // An unauthenticated ordering hint for stale advertisements. The SSH host
    // key and share binding remain the authority for accepting a candidate.
    uint64_t publishedMilliseconds = (uint64_t)(NSDate.date.timeIntervalSince1970 * 1000.0);
    return @{
        @"0001": @[UUIDValue(ServiceUUID)],
        @"0004": @[
            @[[IOBluetoothSDPUUID uuid16:0x0100]],
            @[[IOBluetoothSDPUUID uuid16:0x0003], @{@"DataElementType": @1, @"DataElementSize": @1, @"DataElementValue": @(channel)}]
        ],
        @"0100": @"blugit",
        @"0200": PublicationGenerationValue(publishedMilliseconds),
        @"0201": token
    };
}

#ifdef BLUGIT_TEST_TRANSPORT
// Decode through the SDK to catch a wrong SDP type, width, or NSNumber
// conversion without publishing a service or initializing the radio.
uint64_t blugit_test_sdp_publication_generation(uint32_t *type, uint32_t *count) {
    @autoreleasepool {
        IOBluetoothSDPDataElement *element = [IOBluetoothSDPDataElement withElementValue:ServiceRecord(10, UUIDValue(NSUUID.UUID.UUIDString))[@"0200"]];
        *type = element.getTypeDescriptor;
        *count = (uint32_t)element.getArrayValue.count;
        return ReadPublicationGeneration(element);
    }
}

uint64_t blugit_test_sdp_generation_fixture(uint32_t fixture) {
    @autoreleasepool {
        id high = SDPUnsigned32(417), low = SDPUnsigned32(UINT32_C(3152440429));
        id narrow = @{@"DataElementType": @1, @"DataElementSize": @2, @"DataElementValue": @417};
        id signedPart = @{@"DataElementType": @2, @"DataElementSize": @4, @"DataElementValue": @417};
        id value;
        switch (fixture) {
            case 0: value = @[high, low]; break; // 1794153802861 ms, asymmetric halves.
            case 1: value = @[SDPUnsigned32(UINT32_MAX), SDPUnsigned32(UINT32_MAX)]; break;
            case 2: value = @[]; break;
            case 3: value = @[high]; break;
            case 4: value = @[high, low, low]; break;
            case 5: value = @{@"DataElementType": @1, @"DataElementSize": @8, @"DataElementValue": @1794153802861ULL}; break;
            case 6: value = @[narrow, low]; break;
            case 7: value = @[high, narrow]; break;
            case 8: value = @[signedPart, low]; break;
            case 9: value = @[high, signedPart]; break;
            case 10: value = @[@[high], low]; break;
            default: return 0;
        }
        return ReadPublicationGeneration([IOBluetoothSDPDataElement withElementValue:value]);
    }
}
#endif

#ifdef BLUGIT_TEST_TRANSPORT
// These fixtures only construct in-memory SDK values. They never publish a
// service, register a notification, initialize the radio, or remove a service.
int blugit_test_sdp_channel_fixture(uint32_t fixture) {
    @autoreleasepool {
        id l2cap = @[[IOBluetoothSDPUUID uuid16:0x0100]];
        id uuid = [IOBluetoothSDPUUID uuid16:0x0003];
        id number = @{@"DataElementType": @1, @"DataElementSize": @1, @"DataElementValue": @1};
        id value;
        switch (fixture) {
            case 0: value = @[l2cap, @[uuid, number]]; break;
            case 1: value = @[l2cap, @[uuid, SDPUnsigned32(30)]]; break;
            case 2: value = @[l2cap, @[uuid, @{@"DataElementType": @1, @"DataElementSize": @2, @"DataElementValue": @13}]]; break;
            case 3: value = nil; break;
            case 4: value = @7; break;
            case 5: value = @[l2cap, @[uuid]]; break;
            case 6: value = @[l2cap, @[uuid, SDPUnsigned32(0)]]; break;
            case 7: value = @[l2cap, @[uuid, SDPUnsigned32(31)]]; break;
            case 8: value = @[l2cap, @[uuid, @{@"DataElementType": @1, @"DataElementSize": @4, @"DataElementValue": @(-1)}]]; break;
            case 9: value = @[l2cap, @[uuid, @{@"DataElementType": @2, @"DataElementSize": @4, @"DataElementValue": @3}]]; break;
            case 10: value = @[l2cap, @[uuid, @"3"]]; break;
            case 11: case 12: case 19: {
                id raw = fixture == 11 ? @2.5 : fixture == 12 ? @YES : NSNull.null;
                IOBluetoothSDPDataElement *bad = [IOBluetoothSDPDataElement withType:1 sizeDescriptor:2 size:4 value:raw];
                IOBluetoothSDPDataElement *uuidElement = [IOBluetoothSDPDataElement withElementValue:uuid];
                IOBluetoothSDPDataElement *descriptor = [IOBluetoothSDPDataElement withType:6 sizeDescriptor:5 size:0 value:@[uuidElement, bad]];
                IOBluetoothSDPDataElement *root = [IOBluetoothSDPDataElement withType:6 sizeDescriptor:5 size:0 value:@[descriptor]];
                BluetoothRFCOMMChannelID channel = 0xfe;
                BOOL valid = ReadRFCOMMChannel(root, &channel);
                return valid ? channel : channel == 0xfe ? 0 : -2;
            }
            case 13: value = @[l2cap, @[uuid, @{@"DataElementType": @1, @"DataElementSize": @8, @"DataElementValue": @3}]]; break;
            case 14: value = @[l2cap, @[@3, number]]; break;
            case 15: value = @[l2cap]; break;
            case 16: value = @[l2cap, @[uuid, number], @[uuid, number]]; break;
            case 17: value = @[l2cap, @[uuid, number, number]]; break;
            case 18: value = @[l2cap, @[[IOBluetoothSDPUUID uuid16:4], number]]; break;
            case 20: value = @[@3]; break;
            case 21: value = @[]; break;
            case 22: value = @[l2cap, @[]]; break;
            case 23: value = @[l2cap, @[[uuid getUUIDWithLength:16], SDPUnsigned32(13)]]; break;
            case 24: value = @[l2cap, @[uuid, SDPUnsigned32(UINT32_MAX)]]; break;
            case 25: {
                NSMutableArray *many = [NSMutableArray new];
                for (int i = 0; i < 17; ++i) [many addObject:l2cap];
                [many addObject:@[uuid, number]];
                value = many; break;
            }
            default: return -1;
        }
        BluetoothRFCOMMChannelID channel = 0xfe;
        IOBluetoothSDPDataElement *element = value ? [IOBluetoothSDPDataElement withElementValue:value] : nil;
        BOOL valid = ReadRFCOMMChannel(element, &channel);
        return valid ? channel : channel == 0xfe ? 0 : -2;
    }
}

@interface TestPublicationRecord : IOBluetoothSDPServiceRecord
@property(nonatomic) uint32_t removals;
@property(nonatomic, strong) IOBluetoothSDPDataElement *replacementToken;
@end
@implementation TestPublicationRecord
- (IOBluetoothSDPDataElement *)getAttributeDataElement:(BluetoothSDPServiceAttributeID)attribute {
    if (attribute == 0x0201 && self.replacementToken) return self.replacementToken;
    return [super getAttributeDataElement:attribute];
}
- (IOReturn)removeServiceRecord { self.removals += 1; return kIOReturnSuccess; }
@end

uint32_t blugit_test_publication_ownership_fixture(uint32_t fixture) {
    @autoreleasepool {
        IOBluetoothSDPUUID *token = UUIDValue(@"a5eec324-849f-4a9f-90dc-42767a3e104b");
        IOBluetoothSDPUUID *other = UUIDValue(@"74243fef-d7dd-459b-8b7f-106740c49350");
        NSMutableDictionary *definition = [ServiceRecord(1, token) mutableCopy];
        switch (fixture) {
            case 0: break;
            case 1: definition[@"0201"] = other; break;
            case 2: [definition removeObjectForKey:@"0201"]; break;
            case 3: definition[@"0001"] = @[UUIDValue(ServiceUUID), other]; break;
            case 4: definition[@"0001"] = @[other]; break;
            case 5: definition[@"0004"] = ServiceRecord(2, token)[@"0004"]; break;
            case 6: [definition removeObjectForKey:@"0004"]; break;
            case 7: definition[@"0201"] = @"a5eec324-849f-4a9f-90dc-42767a3e104b"; break;
            case 8: definition[@"0001"] = UUIDValue(ServiceUUID); break;
            case 9: break; // Adoption succeeds, then ownership changes before cleanup.
            case 10: token = nil; break;
            case 11: break;
            case 12: definition[@"0004"] = ServiceRecord(0, token)[@"0004"]; break;
            case 13: definition[@"0004"] = @[@[[IOBluetoothSDPUUID uuid16:0x0100]], @[[IOBluetoothSDPUUID uuid16:3]]]; break;
            default: return UINT32_MAX;
        }
        // The record initializer takes decoded numeric attribute keys and SDK
        // elements, unlike the publication factory's plist dictionary format.
        NSMutableDictionary *attributes = [NSMutableDictionary new];
        for (NSString *key in definition) {
            unsigned attribute = 0;
            [[NSScanner scannerWithString:key] scanHexInt:&attribute];
            attributes[@(attribute)] = [IOBluetoothSDPDataElement withElementValue:definition[key]];
        }
        TestPublicationRecord *record = fixture == 11 ? nil : [[TestPublicationRecord alloc] initWithServiceDictionary:attributes device:nil];
        BOOL matches = RecordMatchesPublication(record, token, 1);
        if (fixture == 9) record.replacementToken = [IOBluetoothSDPDataElement withElementValue:other];
        BOOL removed = RemoveOwnedPublication(record, token, 1);
        return (matches ? 1 : 0) | (removed ? 2 : 0) | (record.removals << 8);
    }
}

int blugit_test_notification_filter(int complete, int own, int existing, int expected, int actual, int incoming) {
    return AcceptNotification(complete != 0, own != 0, existing != 0, expected, actual, incoming != 0);
}
#endif

#ifdef BLUGIT_TEST_TRANSPORT
static IOBluetoothSDPServiceRecord *InMemoryRecord(NSDictionary *definition) {
    NSMutableDictionary *attributes = [NSMutableDictionary new];
    for (NSString *key in definition) {
        unsigned attribute = 0;
        [[NSScanner scannerWithString:key] scanHexInt:&attribute];
        attributes[@(attribute)] = [IOBluetoothSDPDataElement withElementValue:definition[key]];
    }
    return [[IOBluetoothSDPServiceRecord alloc] initWithServiceDictionary:attributes device:nil];
}

@interface FakeLocalServiceManager : NSObject
+ (id)sharedDeviceManager;
- (id)getLocalServices;
@end
@implementation FakeLocalServiceManager
+ (id)sharedDeviceManager { return [self new]; }
- (id)getLocalServices { return @{}; }
@end

@interface FakeWrongManager : FakeLocalServiceManager @end
@implementation FakeWrongManager
+ (id)sharedDeviceManager { return [NSObject new]; }
@end
@interface FakeNilManager : FakeLocalServiceManager @end
@implementation FakeNilManager
+ (id)sharedDeviceManager { return nil; }
@end
@interface FakeThrowingManager : FakeLocalServiceManager @end
@implementation FakeThrowingManager
+ (id)sharedDeviceManager { [NSException raise:@"Fixture" format:@"read failed"]; return nil; }
@end
@interface FakeThrowingServices : FakeLocalServiceManager @end
@implementation FakeThrowingServices
- (id)getLocalServices { [NSException raise:@"Fixture" format:@"read failed"]; return nil; }
@end
@interface FakeWrongCollection : FakeLocalServiceManager @end
@implementation FakeWrongCollection
- (id)getLocalServices { return @[]; }
@end
@interface FakeNilCollection : FakeLocalServiceManager @end
@implementation FakeNilCollection
- (id)getLocalServices { return nil; }
@end
@interface FakeMissingServices : NSObject
+ (id)sharedDeviceManager;
@end
@implementation FakeMissingServices
+ (id)sharedDeviceManager { return [self new]; }
@end
@interface FakeWrongSharedSignature : NSObject
+ (uint64_t)sharedDeviceManager;
@end
@implementation FakeWrongSharedSignature
+ (uint64_t)sharedDeviceManager { return 0; }
@end
@interface FakeWrongServicesSignature : NSObject
+ (id)sharedDeviceManager;
- (uint64_t)getLocalServices;
@end
@implementation FakeWrongServicesSignature
+ (id)sharedDeviceManager { return [self new]; }
- (uint64_t)getLocalServices { return 0; }
@end

@interface FakeRemoteRecord : IOBluetoothSDPServiceRecord @end
@implementation FakeRemoteRecord
- (IOBluetoothDevice *)device { return (IOBluetoothDevice *)NSNull.null; }
@end
@interface FakeThrowingRecord : IOBluetoothSDPServiceRecord @end
@implementation FakeThrowingRecord
- (IOBluetoothSDPDataElement *)getAttributeDataElement:(BluetoothSDPServiceAttributeID)attribute {
    (void)attribute;
    [NSException raise:@"Fixture" format:@"read failed"]; return nil;
}
@end

int blugit_test_local_services_fixture(uint32_t fixture, uint32_t *used) {
    @autoreleasepool {
        switch (fixture) {
            case 0: return UsedChannelsFromManager(FakeLocalServiceManager.class, used);
            case 1: return UsedChannelsFromManager(Nil, used);
            case 2: return UsedChannelsFromManager(NSObject.class, used);
            case 3: return UsedChannelsFromManager(FakeWrongSharedSignature.class, used);
            case 4: return UsedChannelsFromManager(FakeWrongManager.class, used);
            case 5: return UsedChannelsFromManager(FakeNilManager.class, used);
            case 6: return UsedChannelsFromManager(FakeMissingServices.class, used);
            case 7: return UsedChannelsFromManager(FakeWrongServicesSignature.class, used);
            case 8: return UsedChannelsFromManager(FakeWrongCollection.class, used);
            case 9: return UsedChannelsFromManager(FakeNilCollection.class, used);
            case 10: return UsedChannelsFromManager(FakeThrowingServices.class, used);
            case 11: return UsedChannelsFromManager(FakeThrowingManager.class, used);
        }
        @try {
            IOBluetoothSDPUUID *token = UUIDValue(@"3199a220-1f9e-4f15-9d97-5f7d6b801330");
            id records;
            switch (fixture) {
                case 12: case 22: {
                    NSMutableDictionary *many = [NSMutableDictionary new];
                    id record = InMemoryRecord(@{});
                    for (unsigned i = 0; i < (fixture == 12 ? 1025 : 1024); ++i) many[@(i)] = record;
                    records = many; break;
                }
                case 13: records = @{@1:NSNull.null}; break;
                case 14: records = @{@1:[[FakeRemoteRecord alloc] initWithServiceDictionary:@{} device:nil]}; break;
                case 15: {
                    NSMutableDictionary *foreign = [ServiceRecord(3, token) mutableCopy];
                    // Another service class is only an in-memory SDK object,
                    // never published. Its number must be avoided as well.
                    foreign[@"0001"] = @[token];
                    records = @{@1:InMemoryRecord(ServiceRecord(1, token)),
                                @2:InMemoryRecord(ServiceRecord(13, token)),
                                @3:InMemoryRecord(foreign),
                                @4:InMemoryRecord(@{@"0001":@[token]}),
                                @5:InMemoryRecord(@{@"0004":@[@[[IOBluetoothSDPUUID uuid16:0x0100]]]}),
                                @6:InMemoryRecord(ServiceRecord(13, token))};
                    break;
                }
                case 16: records = @{@1:InMemoryRecord(@{@"0004":@3})}; break;
                // Exact structures read back from our earlier zero/omit
                // probes: no usable port, not an unknown malformed record.
                case 17: records = @{@1:InMemoryRecord(@{@"0004":@[@[[IOBluetoothSDPUUID uuid32:0x0100]], @[[IOBluetoothSDPUUID uuid32:3]]]})}; break;
                case 18: records = @{@1:InMemoryRecord(ServiceRecord(0, token))}; break;
                case 19: records = @{@1:[[FakeThrowingRecord alloc] initWithServiceDictionary:@{} device:nil]}; break;
                case 20: records = @[@1]; break;
                case 21: records = @{@1:InMemoryRecord(ServiceRecord(3, token)), @2:InMemoryRecord(ServiceRecord(31, token))}; break;
                case 23: records = @{@1:InMemoryRecord(@{})}; break;
                case 24: records = @{@1:InMemoryRecord(@{@"0004":@[@[[IOBluetoothSDPUUID uuid16:3], @{@"DataElementType":@2, @"DataElementSize":@4, @"DataElementValue":@0}]]})}; break;
                case 25: records = @{@1:InMemoryRecord(@{@"0004":@[@[[IOBluetoothSDPUUID uuid16:3]], @[[IOBluetoothSDPUUID uuid16:3], SDPUnsigned32(1)]]})}; break;
                case 26: records = @{@1:InMemoryRecord(ServiceRecord(-1, token))}; break;
                case 27: records = @{@1:InMemoryRecord(ServiceRecord(31, token))}; break;
                case 28: records = @{@1:InMemoryRecord(@{@"0004":@[@[[IOBluetoothSDPUUID uuid16:3], @{@"DataElementType":@1, @"DataElementSize":@8, @"DataElementValue":@0}]]})}; break;
                case 29: records = @{@1:InMemoryRecord(@{@"0004":@[@[[IOBluetoothSDPUUID uuid16:3], @"0"]]})}; break;
                case 30: records = @{@1:InMemoryRecord(ServiceRecord(0, token)),
                                    @2:InMemoryRecord(@{@"0004":@[@[[IOBluetoothSDPUUID uuid32:0x0100]], @[[IOBluetoothSDPUUID uuid32:3]]]}),
                                    @3:InMemoryRecord(ServiceRecord(13, token))}; break;
                case 31: records = @{@1:InMemoryRecord(@{@"0004":@[@[[IOBluetoothSDPUUID uuid32:0x0100]], @[[IOBluetoothSDPUUID uuid32:3], SDPUnsigned32(0)]]})}; break;
                case 32: records = @{@1:InMemoryRecord(@{@"0004":@[@[[IOBluetoothSDPUUID uuid16:3], SDPUnsigned32(0)], @[[IOBluetoothSDPUUID uuid16:3], SDPUnsigned32(0)]]})}; break;
                default: return -1;
            }
            return ChannelsFromLocalServices(records, used);
        } @catch (NSException *exception) {
            (void)exception;
            return LocalServicesException;
        }
    }
}

int blugit_test_local_getter_signature(uint32_t fixture) {
    @autoreleasepool {
        const char *encoding;
        switch (fixture) {
            case 0: encoding = "@16@0:8"; break;
            case 1: encoding = "@16#0:8"; break;
            case 2: encoding = "Q16@0:8"; break;
            case 3: encoding = "v16@0:8"; break;
            case 4: encoding = "@20@0:8i16"; break;
            case 5: encoding = "@16^v0:8"; break;
            case 6: encoding = "@16@0^v8"; break;
            case 7: encoding = "@?16@0:8"; break;
            case 8: return ValidLocalGetterSignature(nil);
            default: return -1;
        }
        return ValidLocalGetterSignature([NSMethodSignature signatureWithObjCTypes:encoding]);
    }
}
#endif

@interface SDPQuery : NSObject
@property(nonatomic, strong) IOBluetoothDevice *device;
@property(atomic) BOOL done;
@property(atomic) IOReturn status;
@end
@implementation SDPQuery
- (void)sdpQueryComplete:(IOBluetoothDevice *)device status:(IOReturn)status {
    if (device != self.device) return;
    self.status = status;
    self.done = YES;
}
@end

// IOBluetooth stores the query target unretained. A timeout does not cancel
// callbacks, so keep it alive until this one-shot helper process exits.
static SDPQuery *ActiveSDPQuery;

static void DiscoverChannels(NSString *address) {
    IOBluetoothDevice *device = [IOBluetoothDevice deviceWithAddressString:address];
    ActiveSDPQuery = [SDPQuery new];
    SDPQuery *query = ActiveSDPQuery;
    query.device = device;
    uuid_t bytes;
    [[[NSUUID alloc] initWithUUIDString:ServiceUUID] getUUIDBytes:bytes];
    NSArray *uuids = @[[IOBluetoothSDPUUID uuidWithBytes:bytes length:16]];
    IOReturn status = device ? [device performSDPQuery:query uuids:uuids] : kIOReturnBadArgument;
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:6];
    while (status == kIOReturnSuccess && !query.done && deadline.timeIntervalSinceNow > 0) {
        [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.05]];
    }
    uint64_t generation[31] = {0};
    BOOL seen[31] = {NO};
    if (status == kIOReturnSuccess && query.done && query.status == kIOReturnSuccess) {
        for (IOBluetoothSDPServiceRecord *record in device.services) {
            BluetoothRFCOMMChannelID channel = 0;
            if (![record matchesUUIDArray:uuids] || !ReadRFCOMMChannel([record getAttributeDataElement:0x0004], &channel)) continue;
            IOBluetoothSDPDataElement *element = [record getAttributeDataElement:0x0200];
            uint64_t value = ReadPublicationGeneration(element);
            seen[channel] = YES;
            generation[channel] = MAX(generation[channel], value);
        }
    } else if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS")) {
        fprintf(stderr, "blugit Bluetooth: SDP query incomplete start=0x%x done=%d status=0x%x\n", status, query.done, query.status);
    }
    // One bounded line, at most 30 unique valid channels. These are hints only.
    printf("CHANNELS");
    int candidateCount = 0;
    for (int count = 0; count < 30; ++count) {
        int selected = 0;
        for (int channel = 1; channel <= 30; ++channel) {
            if (seen[channel] && (!selected || generation[channel] > generation[selected])) selected = channel;
        }
        if (!selected) break;
        printf("\t%d", selected);
        seen[selected] = NO;
        ++candidateCount;
    }
    puts("");
    if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS")) fprintf(stderr, "blugit Bluetooth: SDP discovery returned %d candidate(s)\n", candidateCount);
}

int blugit_macos_main(int argc, const char **argv) {
    @autoreleasepool {
        signal(SIGPIPE, SIG_IGN);
        setvbuf(stdout, NULL, _IOLBF, 0);
        if (argc < 1) return 2;
        InstallTermination();
        LiveBridges = [NSMutableSet new];
        // Cancellation also applies while AppKit/TCC is still initializing.
        MonitorParent();
        __attribute__((objc_precise_lifetime)) CBCentralManager *power = WaitForBluetooth();
        NSString *mode = @(argv[0]);
        NSString *address = IOBluetoothHostController.defaultController.addressAsString;
        if (!address) Fail(@"No local Bluetooth address");
        if ([mode isEqualToString:@"address"]) { puts(address.UTF8String); return 0; }
        if ([mode isEqualToString:@"discover"] && argc == 2) { DiscoverChannels(@(argv[1])); return 0; }
        if ([mode isEqualToString:@"host"] && argc == 4) {
            int requested = atoi(argv[2]);
            if (requested < 0 || requested > 30) Fail(@"Invalid RFCOMM channel");
            NSString *notificationMode = @(argv[3]);
            if (![notificationMode isEqualToString:@"channel"] && ![notificationMode isEqualToString:@"all"]) Fail(@"Invalid RFCOMM notification mode");
            __attribute__((objc_precise_lifetime)) Listener *listener = [Listener new];
            listener.path = @(argv[1]);
            listener.existingChannels = [NSMutableSet new];
            uint32_t used = UsedLocalChannels();
            if (requested && (used & (UINT32_C(1) << requested))) {
                Fail(@"Requested RFCOMM channel already has a local advertisement, including a retired service; omit --channel to choose an unused channel");
            }
            // The reservation namespace is independent of BLUGIT_HOME and the
            // helper binary cache. Never unlink its files: an existing inode
            // may still be locked by another process.
            NSString *directory = [[NSHomeDirectory() stringByAppendingPathComponent:@"Library/Caches/blugit/channels"] stringByAppendingPathComponent:address];
            NSError *directoryError = nil;
            if (![[NSFileManager defaultManager] createDirectoryAtPath:directory withIntermediateDirectories:YES attributes:@{NSFilePosixPermissions: @0700} error:&directoryError]) {
                Fail([NSString stringWithFormat:@"Cannot create RFCOMM reservation directory: %@", directoryError.localizedDescription]);
            }
            BluetoothRFCOMMChannelID reserved = 0;
            for (;;) {
                int candidate = 0;
                int reservation = ReserveChannel(directory.fileSystemRepresentation, requested, used, &candidate);
                if (reservation < 0) {
                    if (errno == EADDRINUSE) Fail(requested ? @"Requested RFCOMM channel is unavailable; omit --channel for automatic allocation" : @"No free RFCOMM channel is available");
                    Fail([NSString stringWithFormat:@"Cannot reserve RFCOMM channel: %s", strerror(errno)]);
                }
                // A process may have published and exited since the first
                // inventory. Recheck while holding this number's lock, so no
                // cooperating blugit process can publish it in the gap.
                used |= UsedLocalChannels();
                if (used & (UINT32_C(1) << candidate)) {
                    close(reservation);
                    if (requested) Fail(@"Requested RFCOMM channel now has a local advertisement; omit --channel to choose an unused channel");
                    continue;
                }
                IOBluetoothSDPUUID *token = UUIDValue(NSUUID.UUID.UUIDString);
                IOBluetoothSDPServiceRecord *record = [IOBluetoothSDPServiceRecord publishedServiceRecordWithDictionary:ServiceRecord(candidate, token)];
                if (RecordMatchesPublication(record, token, candidate)) {
                    PublishedRecord = record;
                    PublishedToken = token;
                    PublishedChannel = reserved = (BluetoothRFCOMMChannelID)candidate;
                    ChannelReservation = reservation;
                    atexit(ReleaseReservation);
                    break;
                }
                // A factory can return a pre-existing service. Never adopt or
                // remove any return value that fails our complete identity.
                fprintf(stderr, "blugit Bluetooth: publication identity mismatch channel=%d; returned record left untouched\n", candidate);
                close(reservation);
                Fail(@"Cannot verify the new RFCOMM publication; returned record left untouched");
            }
            if (!PublishedRecord) Fail(@"Cannot allocate a free RFCOMM channel");
            listener.expected = reserved;
            if (getenv("BLUGIT_BLUETOOTH_DIAGNOSTICS")) {
                BluetoothSDPServiceRecordHandle handle = 0;
                IOReturn status = [PublishedRecord getServiceRecordHandle:&handle];
                uint64_t generation = ReadPublicationGeneration([PublishedRecord getAttributeDataElement:0x0200]);
                fprintf(stderr, "blugit Bluetooth: published channel=%u record=0x%x generation=%llu status=0x%x notifications=%s token=%s\n", reserved, handle, (unsigned long long)generation, status, notificationMode.UTF8String, PublishedToken.description.UTF8String);
            }
            if ([notificationMode isEqualToString:@"all"]) {
                PublishedNotification = [IOBluetoothRFCOMMChannel registerForChannelOpenNotifications:listener selector:@selector(opened:channel:)];
            } else {
                PublishedNotification = [IOBluetoothRFCOMMChannel registerForChannelOpenNotifications:listener selector:@selector(opened:channel:) withChannelID:reserved direction:kIOBluetoothUserNotificationChannelDirectionIncoming];
            }
            if (!PublishedNotification) { ReleaseService(); Fail(@"Cannot listen on allocated RFCOMM channel"); }
            listener.registrationComplete = YES;
            printf("READY\t%s\t%d\n", address.UTF8String, reserved);
            for (;;) {
                @autoreleasepool { [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.1]]; }
                if (power.state != CBManagerStatePoweredOn) { ReleaseService(); Fail(@"Bluetooth stopped"); }
            }
        }
        if ([mode isEqualToString:@"connect"] && argc == 4) {
            Guest = YES; puts("POWER");
            int number = atoi(argv[2]);
            if (number < 1 || number > 30) Fail(@"A direct RFCOMM channel is required");
            IOBluetoothDevice *device = [IOBluetoothDevice deviceWithAddressString:@(argv[1])];
            if (!device) Fail(@"Invalid remote Bluetooth address");
            IOReturn status = [device openConnection];
            if (status != kIOReturnSuccess) Fail([NSString stringWithFormat:@"Baseband connection failed: 0x%x", status]);
            Bridge *bridge = [Bridge new];
            IOBluetoothRFCOMMChannel *channel = nil;
            status = [device openRFCOMMChannelSync:&channel withChannelID:(BluetoothRFCOMMChannelID)number delegate:bridge];
            if (status != kIOReturnSuccess || !channel) Fail([NSString stringWithFormat:@"RFCOMM connection failed: 0x%x (channel %d)", status, number]);
            if (![bridge attach:channel path:@(argv[3])]) Fail(@"Could not attach local byte stream");
            puts("READY");
            for (;;) {
                @autoreleasepool { [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.1]]; }
            }
        }
        return 2;
    }
}
