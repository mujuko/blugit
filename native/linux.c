/* Thin BlueZ SDP bridge. Stream I/O and the application live in Rust. */
#include <bluetooth/bluetooth.h>
#include <bluetooth/sdp.h>
#include <bluetooth/sdp_lib.h>
#include <errno.h>
#include <sys/socket.h>
#include <sys/time.h>

int blugit_sdp_channel(const unsigned char *address, const unsigned char *service) {
    bdaddr_t remote = {{0}}, local = {{0}};
    for (int i = 0; i < 6; ++i) remote.b[i] = address[5 - i];
    sdp_session_t *session = sdp_connect(&local, &remote, SDP_RETRY_IF_BUSY);
    if (!session) return -errno;
    struct timeval timeout = {20, 0};
    setsockopt(sdp_get_socket(session), SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
    setsockopt(sdp_get_socket(session), SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout));
    uuid_t uuid;
    sdp_uuid128_create(&uuid, service);
    sdp_list_t *search = sdp_list_append(NULL, &uuid);
    uint32_t range = 0x0000ffff;
    sdp_list_t *attributes = sdp_list_append(NULL, &range);
    sdp_list_t *records = NULL;
    int channel = -ENOENT;
    if (sdp_service_search_attr_req(session, search, SDP_ATTR_REQ_RANGE, attributes, &records) == 0) {
        for (sdp_list_t *item = records; item; item = item->next) {
            sdp_record_t *record = item->data;
            sdp_list_t *protocols = NULL;
            if (sdp_get_access_protos(record, &protocols) == 0) {
                int candidate = sdp_get_proto_port(protocols, RFCOMM_UUID);
                if (candidate >= 1 && candidate <= 30) channel = candidate;
                for (sdp_list_t *p = protocols; p; p = p->next) sdp_list_free(p->data, NULL);
                sdp_list_free(protocols, NULL);
            }
            sdp_record_free(record);
        }
    } else {
        channel = -errno;
    }
    sdp_list_free(records, NULL);
    sdp_list_free(search, NULL);
    sdp_list_free(attributes, NULL);
    sdp_close(session);
    return channel;
}
