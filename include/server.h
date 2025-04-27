// server.h
// Server interface for Argon

#ifndef SERVER_H
#define SERVER_H
#define THREAD_POOL_SIZE 4
#define DEFAULT_LISTEN_ADD "0.0.0.0"

#include <signal.h>
#include "../include/thread_pool.h"

typedef struct {
    int server_fd;
    int epoll_fd;
    int shutdown_fd;
    int opt;
    ThreadPool* pool;
    volatile sig_atomic_t keep_running;
} Server;

Server* server_init(const char* bind_addr, int port);
void server_run(Server* server);
void server_shutdown(Server* server);
void server_handle_signal(int signal, Server* server);


#endif  // SERVER_H
