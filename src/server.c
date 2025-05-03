// server.c
// Server setup and accept loop

#include <arpa/inet.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>
#include <sys/epoll.h>
#include <sys/eventfd.h>
#include <errno.h>


#include "../include/server.h"
#include "../include/http.h"

Server* server_init(ServerConfig* config,  void (*handler)(int, ServerConfig*)){
  Server* server = calloc(1, sizeof(Server));
  if (!server) {
      perror("Failed to allocate memory for Server");
      exit(EXIT_FAILURE);
  }

  // Step 1: Extract unique ports from ServerConfig
  int max_ports = 1024;  // Assume max 1024 unique ports
  int* unique_ports = calloc(max_ports, sizeof(int));
  int port_count = 0;
  ServerName *s, *tmp;
  HASH_ITER(hh, config->servers, s, tmp) {
      int found = 0;
      for (int i = 0; i < port_count; i++) {
          if (unique_ports[i] == s->listen) {
              found = 1;
              break;
          }
      }
      if (!found && port_count < max_ports) {
          unique_ports[port_count++] = s->listen;
      }
  }

  // Step 2: Allocate ListenSocket array
  server->listen_sockets = calloc(port_count, sizeof(ListenSocket));
  server->listen_socket_count = port_count;

  // Step 3: Create listening sockets for each unique port
  server->epoll_fd = epoll_create1(0);
  if (server->epoll_fd == -1) {
      perror("epoll_create1 failed");
      free(unique_ports);
      free(server);
      exit(EXIT_FAILURE);
  }

  for (int i = 0; i < port_count; i++) {
      int port = unique_ports[i];

      // Create socket
      int listen_fd = socket(AF_INET, SOCK_STREAM, 0);
      if (listen_fd == -1) {
          perror("Socket creation failed");
          free(unique_ports);
          free(server);
          exit(EXIT_FAILURE);
      }

      // Set socket options
      server->opt = 1;
      if (setsockopt(listen_fd, SOL_SOCKET, SO_REUSEADDR, &server->opt, sizeof(server->opt)) == -1) {
          perror("Failed to set socket options");
          close(listen_fd);
          free(unique_ports);
          free(server);
          exit(EXIT_FAILURE);
      }

      // Prepare sockaddr_in
      struct sockaddr_in server_addr;
      memset(&server_addr, 0, sizeof(server_addr));
      server_addr.sin_family = AF_INET;
      server_addr.sin_port = htons(port);
      server_addr.sin_addr.s_addr = INADDR_ANY;  // 0.0.0.0

      // Bind socket
      if (bind(listen_fd, (struct sockaddr*)&server_addr, sizeof(server_addr)) == -1) {
          perror("Bind failed");
          close(listen_fd);
          free(unique_ports);
          free(server);
          exit(EXIT_FAILURE);
      }

      // Listen
      if (listen(listen_fd, 128) == -1) {
          perror("Listen failed");
          close(listen_fd);
          free(unique_ports);
          free(server);
          exit(EXIT_FAILURE);
      }

      // Add to epoll
      struct epoll_event ev;
      ev.events = EPOLLIN;
      ev.data.fd = listen_fd;
      if (epoll_ctl(server->epoll_fd, EPOLL_CTL_ADD, listen_fd, &ev) == -1) {
          perror("epoll_ctl failed");
          close(listen_fd);
          free(unique_ports);
          free(server);
          exit(EXIT_FAILURE);
      }

      // Store in ListenSocket
      server->listen_sockets[i].fd = listen_fd;
      server->listen_sockets[i].port = port;
  }

  // Free temporary unique_ports array
  free(unique_ports);

  // Step 4: Initialize shutdown_fd
  server->shutdown_fd = eventfd(0, EFD_NONBLOCK);
  if (server->shutdown_fd == -1) {
      perror("eventfd failed");
      for (int i = 0; i < server->listen_socket_count; i++) {
          close(server->listen_sockets[i].fd);
      }
      free(server->listen_sockets);
      close(server->epoll_fd);
      free(server);
      exit(EXIT_FAILURE);
  }

  struct epoll_event ev;
  ev.events = EPOLLIN;
  ev.data.fd = server->shutdown_fd;
  if (epoll_ctl(server->epoll_fd, EPOLL_CTL_ADD, server->shutdown_fd, &ev) == -1) {
      perror("epoll_ctl shutdown_fd failed");
      for (int i = 0; i < server->listen_socket_count; i++) {
          close(server->listen_sockets[i].fd);
      }
      free(server->listen_sockets);
      close(server->epoll_fd);
      close(server->shutdown_fd);
      free(server);
      exit(EXIT_FAILURE);
  }

  // Step 5: Initialize thread pool
  server->pool = create_thread_pool(4, 1024, config);
  server->handler = handle_client;
  thread_pool_start(server->pool, server->handler);
  server->keep_running = 1;
  server->config = config;

  printf("Server initialized successfully with %d listening sockets\n", port_count);
  return server;
}

void server_run(Server* server) {
  if (!server) return;

  struct epoll_event events[MAX_EVENTS];

  while (server->keep_running) {
      int n = epoll_wait(server->epoll_fd, events, MAX_EVENTS, -1);
      if (n == -1) {
          if (errno == EINTR) continue;
          perror("epoll_wait failed");
          break;
      }

      for (int i = 0; i < n; i++) {
          int event_fd = events[i].data.fd;

          if (event_fd == server->shutdown_fd) {
              printf("Shutdown event received. Shutting down server...\n");
              uint64_t u;
              if (read(server->shutdown_fd, &u, sizeof(u)) != sizeof(u)) {
                  perror("read shutdown_fd failed");
              }
              server_shutdown(server);
              return;
          } else {
              // Check if it's a listening socket
              for (int j = 0; j < server->listen_socket_count; j++) {
                  if (event_fd == server->listen_sockets[j].fd) {
                      struct sockaddr_in client_addr;
                      socklen_t client_len = sizeof(client_addr);
                      int client_fd = accept(event_fd, (struct sockaddr*)&client_addr, &client_len);
                      if (client_fd == -1) {
                          perror("accept failed");
                          continue;
                      }

                      printf("Accepted new client %s:%d on port %d\n",
                             inet_ntoa(client_addr.sin_addr),
                             ntohs(client_addr.sin_port),
                             server->listen_sockets[j].port);

                      if (thread_pool_add_task(server->pool, client_fd) != 0) {
                          perror("Failed to add client to thread pool");
                          close(client_fd);
                      }
                      break;
                  }
              }
          }
      }
  }

  printf("Server run loop exited.\n");
}

void server_shutdown(Server* server) {
  if (!server) return;

  printf("Shutting down server...\n");

  server->keep_running = 0;

  // Close all listening sockets
  for (int i = 0; i < server->listen_socket_count; i++) {
      if (server->listen_sockets[i].fd != -1) {
          close(server->listen_sockets[i].fd);
      }
  }
  free(server->listen_sockets);

  if (server->epoll_fd != -1) {
      close(server->epoll_fd);
      server->epoll_fd = -1;
  }

  if (server->shutdown_fd != -1) {
      close(server->shutdown_fd);
      server->shutdown_fd = -1;
  }

  if (server->pool) {
      thread_pool_destroy(server->pool);
      server->pool = NULL;
  }

  free(server);

  printf("Server shutdown complete.\n");
}
