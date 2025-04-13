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
static int server_fd_global = -1;

void handle_signal(int signal) {
  if (signal == SIGINT || signal == SIGTERM) {
    printf("Shutdown signal received. Setting keep_running to 0...\n");
    keep_running = 0;
    if (server_fd_global != -1) {
      printf("Closing server socket to interrupt accept...\n");
      close(server_fd_global);
      server_fd_global = -1;
    }
  }
}

void fatal(const char *message) {
  perror(message);
  exit(EXIT_FAILURE);
}

void shutdown_server(int server_fd) {
  printf("Entering shutdown_server...\n");
  keep_running = 0;

  if (server_fd != -1 && server_fd_global != -1) {
    printf("Closing server socket in shutdown_server...\n");
    close(server_fd);
  }

  printf("Broadcasting to wake worker threads...\n");
  pthread_mutex_lock(&queue_mutex);
  pthread_cond_broadcast(&queue_cond);
  pthread_mutex_unlock(&queue_mutex);

  printf("Calling thread_pool_destroy...\n");
  thread_pool_destroy();

  printf("Server shutdown complete.\n");
}

void start_server() {
  int server_fd;
  struct sockaddr_in server_addr;

  printf("Initializing thread pool...\n");
  thread_pool_init(THREAD_POOL_SIZE);

  printf("Creating socket...\n");
  server_fd = socket(AF_INET, SOCK_STREAM, 0);
  if (server_fd == -1) {
    fatal("socket failed");
  }
  server_fd_global = server_fd;

  int opt = ENABLE_SO_REUSEADDR;
  if (setsockopt(server_fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt)) == -1) {
    fatal("setsockopt failed");
  }

  memset(&server_addr, 0, sizeof(server_addr));
  server_addr.sin_family = AF_INET;
  server_addr.sin_addr.s_addr = INADDR_ANY;
  server_addr.sin_port = htons(SERVER_PORT);

  if (bind(server_fd, (struct sockaddr *)&server_addr, sizeof(server_addr)) == -1) {
    fatal("bind failed");
  }

  printf("Socket successfully bound to port %d\n", SERVER_PORT);

  if (listen(server_fd, BACKLOG) == -1) {
    fatal("listen failed");
  }

  printf("Listening for incoming connections...\n");

  while (keep_running) {
    printf("KEEP: %d\n", keep_running);
    struct sockaddr_in client_addr;
    socklen_t client_len = sizeof(client_addr);

    int client_fd = accept(server_fd, (struct sockaddr *)&client_addr, &client_len);
    if (client_fd < 0) {
      if (!keep_running) {
        printf("Accept interrupted due to shutdown.\n");
        break;
      }
      perror("accept failed");
      continue;
    }

    printf("Accepted connection from %s:%d\n", inet_ntoa(client_addr.sin_addr),
           ntohs(client_addr.sin_port));

    thread_pool_add_task(client_fd);
  }

  printf("Exiting accept loop, proceeding to shutdown...\n");
  shutdown_server(server_fd);
}
