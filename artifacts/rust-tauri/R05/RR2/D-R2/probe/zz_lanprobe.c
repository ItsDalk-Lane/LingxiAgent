// Zero-Lingxi-code LAN self-connection probe (RR2 D-R2).
// Usage: zz_lanprobe <bind_addr> <connect_addr> [port]
// - binds <bind_addr>:<port> (default port random-ish 47654)
// - spawns a server thread: accept -> read -> write a fixed response
// - main thread: connect to <connect_addr>:<port>, write request, read
//   response with hard timeouts, print PHASE results.
// Exit prints one line: PROBE RESULT: <outcome>.
#include <arpa/inet.h>
#include <errno.h>
#include <netdb.h>
#include <netinet/in.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <unistd.h>

static int port = 47654;

static void *server_main(void *arg) {
    (void)arg;
    int ls = socket(AF_INET6, SOCK_STREAM, 0);
    if (ls < 0) { printf("server: socket v6 failed\n"); return NULL; }
    int v6only = 0;
    setsockopt(ls, IPPROTO_IPV6, IPV6_V6ONLY, &v6only, sizeof(v6only));
    struct sockaddr_in6 addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin6_family = AF_INET6;
    addr.sin6_port = htons((uint16_t)port);
    addr.sin6_addr = in6addr_any;
    if (bind(ls, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        // fall back to v4 bind
        close(ls);
        ls = socket(AF_INET, SOCK_STREAM, 0);
        struct sockaddr_in a4;
        memset(&a4, 0, sizeof(a4));
        a4.sin_family = AF_INET;
        a4.sin_port = htons((uint16_t)port);
        a4.sin_addr.s_addr = htonl(INADDR_ANY);
        if (bind(ls, (struct sockaddr *)&a4, sizeof(a4)) < 0) {
            printf("server: bind failed: %s\n", strerror(errno));
            return NULL;
        }
    }
    if (listen(ls, 4) < 0) { printf("server: listen failed: %s\n", strerror(errno)); return NULL; }
    struct timeval tv = {12, 0};
    setsockopt(ls, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    printf("server: listening on :%d\n", port);
    int cs = accept(ls, NULL, NULL);
    if (cs < 0) { printf("server: accept failed/timeout: %s\n", strerror(errno)); close(ls); return NULL; }
    printf("server: accepted\n");
    char buf[256];
    ssize_t n = recv(cs, buf, sizeof(buf) - 1, 0);
    if (n < 0) { printf("server: recv failed: %s\n", strerror(errno)); }
    else { buf[n] = 0; printf("server: recv %zd bytes\n", n); }
    const char *resp = "PROBE-RESPONSE";
    ssize_t w = send(cs, resp, strlen(resp), 0);
    printf("server: send %zd\n", w);
    close(cs);
    close(ls);
    return NULL;
}

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: %s <connect_addr> [port]\n", argv[0]);
        return 2;
    }
    const char *connect_addr = argv[1];
    if (argc >= 3) port = atoi(argv[2]);

    pthread_t th;
    pthread_create(&th, NULL, server_main, NULL);
    usleep(200000); // let the listener come up

    struct addrinfo hints, *res = NULL;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    int rc = getaddrinfo(connect_addr, NULL, &hints, &res);
    if (rc != 0 || !res) { printf("PROBE RESULT: resolve-failed %s\n", gai_strerror(rc)); return 1; }
    int fd = socket(res->ai_family, res->ai_socktype, 0);
    if (fd < 0) { printf("PROBE RESULT: socket-failed\n"); return 1; }
    struct timeval tv = {6, 0};
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    if (res->ai_family == AF_INET6) ((struct sockaddr_in6 *)res->ai_addr)->sin6_port = htons((uint16_t)port);
    else ((struct sockaddr_in *)res->ai_addr)->sin_port = htons((uint16_t)port);

    if (connect(fd, res->ai_addr, res->ai_addrlen) < 0) {
        printf("PROBE RESULT: connect-failed %s\n", strerror(errno));
        pthread_join(th, NULL);
        return 1;
    }
    printf("client: connected\n");
    if (send(fd, "PING", 4, 0) < 0) { printf("PROBE RESULT: send-failed %s\n", strerror(errno)); return 1; }
    printf("client: sent\n");
    char buf[256];
    ssize_t n = recv(fd, buf, sizeof(buf) - 1, 0);
    if (n < 0) { printf("PROBE RESULT: recv-stalled %s\n", strerror(errno)); return 1; }
    buf[n] = 0;
    printf("client: recv %zd bytes: %s\n", n, buf);
    printf("PROBE RESULT: ok\n");
    pthread_join(th, NULL);
    return 0;
}
