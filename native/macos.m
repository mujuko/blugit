// The platform boundary only: IOBluetooth callbacks and Unix socket bridging.
// Repository selection, Git framing and all application logic live in Rust.
#import <Foundation/Foundation.h>
#import <CoreFoundation/CoreFoundation.h>
#import <IOBluetooth/IOBluetooth.h>
#import <CoreBluetooth/CoreBluetooth.h>
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
static IOBluetoothUserNotification *PublishedNotification;
static dispatch_source_t TerminationSource;

// IOBluetooth can return the requested channel even when another blugit
// process has already published it. Keep this lock in the native owner, not
// the state directory or Rust parent: listener shutdown may still be draining
// bridges, and a helper from another binary version shares the same adapter.
static int ChannelReservation = -1;

static void ReleaseReservation(void) {
    if (ChannelReservation >= 0) close(ChannelReservation);
    ChannelReservation = -1;
}

static int ReserveChannel(const char *directory, int requested, int first, int *selected) {
    if (requested < 0 || requested > 30 || first < 1 || first > 30) { errno = EINVAL; return -1; }
    for (int channel = requested ? requested : first; channel <= (requested ? requested : 30); ++channel) {
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
    return ReserveChannel(directory, requested, 1, selected);
}
#endif

static void ReleaseService(void) {
    [PublishedNotification unregister]; PublishedNotification = nil;
    [PublishedRecord removeServiceRecord]; PublishedRecord = nil;
}

static void InstallTermination(void) {
    signal(SIGTERM, SIG_IGN);
    dispatch_queue_t queue = dispatch_get_global_queue(QOS_CLASS_UTILITY, 0);
    TerminationSource = dispatch_source_create(DISPATCH_SOURCE_TYPE_SIGNAL, SIGTERM, 0, queue);
    dispatch_source_set_event_handler(TerminationSource, ^{
        dispatch_async(dispatch_get_main_queue(), ^{ ReleaseService(); exit(0); });
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

static CBCentralManager *WaitForBluetooth(void) {
    PowerDelegate *delegate = [PowerDelegate new];
    CBCentralManager *manager = [[CBCentralManager alloc] initWithDelegate:delegate queue:nil options:@{CBCentralManagerOptionShowPowerAlertKey: @NO}];
    // Retain the delegate because CBCentralManager's delegate property is weak.
    static PowerDelegate *retained;
    retained = delegate;
    (void)retained;
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:55];
    while (manager.state == CBManagerStateUnknown || manager.state == CBManagerStateResetting || CBManager.authorization == CBManagerAuthorizationNotDetermined) {
        if (deadline.timeIntervalSinceNow <= 0) Fail(@"Bluetooth initialization timed out");
        [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.05]];
    }
    if (manager.state != CBManagerStatePoweredOn) Fail(@"Bluetooth is off, unavailable, or permission was denied");
    return manager;
}

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
    if (fd < 0) return NO;
    struct sockaddr_un address = {0};
    address.sun_family = AF_UNIX;
    const char *name = path.fileSystemRepresentation;
    if (strlen(name) >= sizeof(address.sun_path)) { close(fd); return NO; }
    strlcpy(address.sun_path, name, sizeof(address.sun_path));
    if (connect(fd, (struct sockaddr *)&address, sizeof(address)) != 0) { close(fd); return NO; }
    int yes = 1;
    setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &yes, sizeof(yes));
    self.fd = fd;
    self.channel = channel;
    self.mtu = MAX(1, MIN(channel.getMTU, 65535));
    self.written = dispatch_semaphore_create(0);
    int outputFD = dup(fd);
    if (outputFD < 0) { close(fd); self.fd = -1; return NO; }
    self.output = dispatch_io_create(DISPATCH_IO_STREAM, outputFD, dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^(int error) { (void)error; close(outputFD); });
    [channel setDelegate:self];
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
            if (count <= 0) {
                int error = count < 0 ? errno : 0;
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
    (void)channel;
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
    (void)channel;
    if (self.output) {
        __weak Bridge *weak = self;
        dispatch_io_barrier(self.output, ^{ dispatch_async(dispatch_get_main_queue(), ^{ [weak finish]; }); });
    } else { [self finish]; }
}
- (void)finish {
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

@interface Listener : NSObject
@property(nonatomic, strong) NSString *path;
@end
@implementation Listener
- (void)opened:(IOBluetoothUserNotification *)notification channel:(IOBluetoothRFCOMMChannel *)channel {
    (void)notification;
    // macOS cannot safely hold two RFCOMM channels to the same device.
    for (Bridge *bridge in LiveBridges) {
        if ([bridge.channel.getDevice.addressString isEqualToString:channel.getDevice.addressString]) {
            [channel closeChannel]; return;
        }
    }
    Bridge *bridge = [Bridge new];
    if (![bridge attach:channel path:self.path]) [channel closeChannel];
}
@end

static NSDictionary *ServiceRecord(int channel) {
    uuid_t bytes;
    [[[NSUUID alloc] initWithUUIDString:ServiceUUID] getUUIDBytes:bytes];
    return @{
        @"0001": @[[IOBluetoothSDPUUID uuidWithBytes:bytes length:16]],
        @"0004": @[
            @[[IOBluetoothSDPUUID uuid16:0x0100]],
            @[[IOBluetoothSDPUUID uuid16:0x0003], @{@"DataElementType": @1, @"DataElementSize": @1, @"DataElementValue": @(channel)}]
        ],
        @"0100": @"blugit"
    };
}

int blugit_macos_main(int argc, const char **argv) {
    @autoreleasepool {
        signal(SIGPIPE, SIG_IGN);
        setvbuf(stdout, NULL, _IOLBF, 0);
        if (argc < 1) return 2;
        InstallTermination();
        LiveBridges = [NSMutableSet new];
        __attribute__((objc_precise_lifetime)) CBCentralManager *power = WaitForBluetooth();
        NSString *mode = @(argv[0]);
        NSString *address = IOBluetoothHostController.defaultController.addressAsString;
        if (!address) Fail(@"No local Bluetooth address");
        if ([mode isEqualToString:@"address"]) { puts(address.UTF8String); return 0; }
        if ([mode isEqualToString:@"host"] && argc == 3) {
            int requested = atoi(argv[2]);
            if (requested < 0 || requested > 30) Fail(@"Invalid RFCOMM channel");
            MonitorParent();
            __attribute__((objc_precise_lifetime)) Listener *listener = [Listener new];
            listener.path = @(argv[1]);
            // The reservation namespace is independent of BLUGIT_HOME and the
            // helper binary cache. Never unlink its files: an existing inode
            // may still be locked by another process.
            NSString *directory = [[NSHomeDirectory() stringByAppendingPathComponent:@"Library/Caches/blugit/channels"] stringByAppendingPathComponent:address];
            NSError *directoryError = nil;
            if (![[NSFileManager defaultManager] createDirectoryAtPath:directory withIntermediateDirectories:YES attributes:@{NSFilePosixPermissions: @0700} error:&directoryError]) {
                Fail([NSString stringWithFormat:@"Cannot create RFCOMM reservation directory: %@", directoryError.localizedDescription]);
            }
            BluetoothRFCOMMChannelID reserved = 0;
            for (int first = 1; first <= 30;) {
                int candidate = 0;
                int reservation = ReserveChannel(directory.fileSystemRepresentation, requested, first, &candidate);
                if (reservation < 0) {
                    if (errno == EADDRINUSE) Fail(requested ? @"Requested RFCOMM channel is unavailable; omit --channel for automatic allocation" : @"No free RFCOMM channel is available");
                    Fail([NSString stringWithFormat:@"Cannot reserve RFCOMM channel: %s", strerror(errno)]);
                }
                PublishedRecord = [IOBluetoothSDPServiceRecord publishedServiceRecordWithDictionary:ServiceRecord(candidate)];
                BOOL valid = PublishedRecord && [PublishedRecord getRFCOMMChannelID:&reserved] == kIOReturnSuccess && reserved == candidate;
                if (valid) { ChannelReservation = reservation; atexit(ReleaseReservation); break; }
                ReleaseService(); close(reservation);
                if (requested) Fail(@"Requested RFCOMM channel is unavailable; omit --channel for automatic allocation");
                first = candidate + 1;
            }
            if (!PublishedRecord) Fail(@"Cannot allocate a free RFCOMM channel");
            PublishedNotification = [IOBluetoothRFCOMMChannel registerForChannelOpenNotifications:listener selector:@selector(opened:channel:) withChannelID:reserved direction:kIOBluetoothUserNotificationChannelDirectionIncoming];
            if (!PublishedNotification) { ReleaseService(); Fail(@"Cannot listen on allocated RFCOMM channel"); }
            printf("READY\t%s\t%d\n", address.UTF8String, reserved);
            for (;;) {
                @autoreleasepool { [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.1]]; }
                if (power.state != CBManagerStatePoweredOn) { ReleaseService(); Fail(@"Bluetooth stopped"); }
            }
        }
        if ([mode isEqualToString:@"connect"] && argc == 4) {
            Guest = YES; MonitorParent(); puts("POWER");
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
