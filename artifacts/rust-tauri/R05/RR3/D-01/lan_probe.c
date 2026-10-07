// 最小网络探针：先完成真实监听，再创建客户端；每个等待都有上限。
#include <arpa/inet.h>
#include <errno.h>
#include <poll.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static int listener;
static int accepted = 0;
static void *serve(void *unused) {
    (void)unused;
    struct pollfd ready = {listener, POLLIN, 0};
    int rc = poll(&ready, 1, 6000);
    printf("server poll=%d revents=%d\n", rc, ready.revents);
    if (rc <= 0) return NULL;
    int peer = accept(listener, NULL, NULL);
    if (peer < 0) { perror("accept"); return NULL; }
    accepted = 1;
    printf("server accepted\n");
    struct timeval budget = {4, 0};
    setsockopt(peer, SOL_SOCKET, SO_RCVTIMEO, &budget, sizeof(budget));
    char data[4];
    ssize_t size = recv(peer, data, sizeof(data), 0);
    printf("server recv=%zd\n", size);
    if (size == 4 && memcmp(data, "PING", 4) == 0) send(peer, "PONG", 4, 0);
    close(peer);
    return NULL;
}
int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc != 2) return 2;
    listener = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in address = {0};
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_ANY);
    if (bind(listener, (struct sockaddr *)&address, sizeof(address)) || listen(listener, 4)) {
        perror("listen"); return 2;
    }
    socklen_t length = sizeof(address);
    getsockname(listener, (struct sockaddr *)&address, &length);
    printf("pid=%d bind=0.0.0.0:%u connect=%s:%u\n", getpid(), ntohs(address.sin_port), argv[1], ntohs(address.sin_port));
    if (inet_pton(AF_INET, argv[1], &address.sin_addr) != 1) return 2;
    pthread_t thread;
    pthread_create(&thread, NULL, serve, NULL);
    int client = socket(AF_INET, SOCK_STREAM, 0);
    struct timeval budget = {4, 0};
    setsockopt(client, SOL_SOCKET, SO_RCVTIMEO, &budget, sizeof(budget));
    setsockopt(client, SOL_SOCKET, SO_SNDTIMEO, &budget, sizeof(budget));
    if (connect(client, (struct sockaddr *)&address, sizeof(address))) { perror("connect"); return 1; }
    printf("client connected\n");
    ssize_t sent = send(client, "PING", 4, 0);
    printf("client send=%zd\n", sent);
    char response[4];
    ssize_t received = recv(client, response, sizeof(response), 0);
    printf("client recv=%zd errno=%d (%s)\n", received, errno, strerror(errno));
    close(client);
    pthread_join(thread, NULL);
    close(listener);
    int ok = accepted && sent == 4 && received == 4 && memcmp(response, "PONG", 4) == 0;
    printf("PROBE RESULT: %s accepted=%d\n", ok ? "ok" : "blocked", accepted);
    return ok ? 0 : 1;
}
