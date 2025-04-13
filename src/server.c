// server.c
// Server setup and accept loop

#include <arpa/inet.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#include "../include/constants.h"
#include "../include/server.h"
#include "../include/thread_pool.h"

volatile sig_atomic_t keep_running = 1;

void handle_signal(int signal) {
  if (signal == SIGINT || signal == SIGTERM) {
    printf("Shutdown signal received. Shutting down server gracefully...\n");
    keep_running = 0;
  }
}

void fatal(const char *message) {
  perror(message);
  exit(EXIT_FAILURE);
}

void shutdown_server(int server_fd) {
  pthread_mutex_lock(&queue_mutex);

  keep_running = 0;

  // Wake up all worker threads waiting for tasks
  pthread_cond_broadcast(&queue_cond);

  pthread_mutex_unlock(&queue_mutex);

  // Destroy thread pool cleanly
  thread_pool_destroy();

  // Close server socket
  close(server_fd);

  printf("Server shutdown complete.\n");
}

void start_server() {
  int server_fd;
  struct sockaddr_in server_addr;
  // Initialize thread pool
  thread_pool_init(THREAD_POOL_SIZE);
  // Create socket
  server_fd = socket(AF_INET, SOCK_STREAM, 0);
  if (server_fd == -1) {
    fatal("socket failed");
  }

  // Set socket options
  int opt = ENABLE_SO_REUSEADDR;
  if (setsockopt(server_fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt)) ==
      -1) {
    fatal("setsockopt failed");
  }

  // Bind socket
  memset(&server_addr, 0, sizeof(server_addr));
  server_addr.sin_family = AF_INET;
  server_addr.sin_addr.s_addr = INADDR_ANY;
  server_addr.sin_port = htons(SERVER_PORT);

  if (bind(server_fd, (struct sockaddr *)&server_addr, sizeof(server_addr)) ==
      -1) {
    fatal("bind failed");
  }

  printf("Socket successfully bound to port %d\n", SERVER_PORT);

  // Listen
  if (listen(server_fd, BACKLOG) == -1) {
    fatal("listen failed");
  }

  printf("Listening for incoming connections...\n");

  // Accept loop
  while (keep_running) {
    struct sockaddr_in client_addr;
    socklen_t client_len = sizeof(client_addr);

    int client_fd =
        accept(server_fd, (struct sockaddr *)&client_addr, &client_len);
    if (client_fd < 0) {
      perror("accept failed");
      continue;
    }

    printf("Accepted connection from %s:%d\n", inet_ntoa(client_addr.sin_addr),
           ntohs(client_addr.sin_port));

    thread_pool_add_task(client_fd);
  }

  shutdown_server(server_fd);
}
