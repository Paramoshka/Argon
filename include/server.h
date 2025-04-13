// server.h
// Server interface for Argon

#ifndef SERVER_H
#define SERVER_H
#define THREAD_POOL_SIZE 4
#include <signal.h>
extern volatile sig_atomic_t keep_running;
void start_server();
void handle_signal(int signal);
#endif  // SERVER_H
