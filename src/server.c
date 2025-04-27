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
#include <sys/epoll.h>
#include <sys/eventfd.h>
#include <errno.h>


#include "../include/server.h"

Server* server_init(const char* bind_addr, int port) {
  Server* server = calloc(1, sizeof(Server));
  if (!server) {
    perror("Failed allocate memry for Server");
    exit(EXIT_FAILURE);
  }

  // set socket
  server->server_fd = socket(AF_INET, SOCK_STREAM, 0);
  if (server->server_fd == -1) {
    perror("socker created failed");
    free(server);
    exit(EXIT_FAILURE);
  }

  // set socket options
  server->opt = 1;

  if (setsockopt(server->server_fd, SOL_SOCKET, SO_REUSEADDR, &server->opt, sizeof(server->opt)) == -1) {
    perror("failed set optionsfor socker");
    close(server->server_fd);
    free(server);
    exit(EXIT_FAILURE);
  }

  // prepae sock addr
  struct sockaddr_in server_addr;

  memset(&server_addr, 0, sizeof(server_addr));

  server_addr.sin_family = AF_INET;
  server_addr.sin_port = htons(port);

  if (!bind_addr) {
    bind_addr = DEFAULT_LISTEN_ADD;
  }

  if (inet_pton(AF_INET, bind_addr, &server_addr.sin_addr) <= 0)
  {
    perror("inet_pton failed for bind address");
    close(server->server_fd);
    free(server);
    exit(EXIT_FAILURE);
  }

  if (bind(server->server_fd, (struct sockaddr*)&server_addr, sizeof(server_addr)) == -1)
  {
    perror("Failed bind server");
    close(server->server_fd);
    free(server);
    exit(EXIT_FAILURE);
  }

  if (listen(server->server_fd, 128) == -1)
  {
    perror("listen failed");
    close(server->server_fd);
    free(server);
    exit(EXIT_FAILURE);
  }

  server->epoll_fd = epoll_create1(0);
  if (server->epoll_fd == -1) {
      perror("epoll_create1 failed");
      close(server->server_fd);
      free(server);
      exit(EXIT_FAILURE);
  }

  struct epoll_event ev;

  ev.events = EPOLLIN;
  ev.data.fd = server->server_fd;

  if (epoll_ctl(server->epoll_fd, EPOLL_CTL_ADD, server->server_fd, &ev) == -1)
  {
    perror("epoll_ctl server_fd failed");
    close(server->server_fd);
    close(server->epoll_fd);
    free(server);
    exit(EXIT_FAILURE);
  }

  server->shutdown_fd = eventfd(0, EFD_NONBLOCK);
  if (server->shutdown_fd == -1) {
    perror("eventfd failed");
    close(server->server_fd);
    close(server->epoll_fd);
    free(server);
    exit(EXIT_FAILURE);
  }

  ev.events = EPOLLIN;
  ev.data.fd = server->shutdown_fd;
  if (epoll_ctl(server->epoll_fd, EPOLL_CTL_ADD, server->shutdown_fd, &ev) == -1) {
      perror("epoll_ctl shutdown_fd failed");
      close(server->server_fd);
      close(server->epoll_fd);
      close(server->shutdown_fd);
      free(server);
      exit(EXIT_FAILURE);
  }

  server->pool = create_thread_pool(4, 1024);
  thread_pool_start(server->pool);

  server->keep_running = 1;
  printf("Server initialized successfully on %s:%d\n", bind_addr, port);

  return server;
}

void server_run(Server* server) {
  if (!server) return;

  struct epoll_event events[MAX_EVENTS];

  while (server->keep_running) {
      int n = epoll_wait(server->epoll_fd, events, MAX_EVENTS, -1);
      if (n == -1) {
          if (errno == EINTR) {
              continue;
          }
          perror("epoll_wait failed");
          break;
      }

      for (int i = 0; i < n; i++) {
          int event_fd = events[i].data.fd;

          if (event_fd == server->server_fd) {
              struct sockaddr_in client_addr;
              socklen_t client_len = sizeof(client_addr);

              int client_fd = accept(server->server_fd, (struct sockaddr*)&client_addr, &client_len);
              if (client_fd == -1) {
                  perror("accept failed");
                  continue;
              }

              printf("Accepted new client %s:%d\n",
                     inet_ntoa(client_addr.sin_addr),
                     ntohs(client_addr.sin_port));

              if (thread_pool_add_task(server->pool, client_fd) != 0) {
                  perror("Failed to add client to thread pool");
                  close(client_fd);
              }

          } else if (event_fd == server->shutdown_fd) {
            printf("Shutdown event received. Shutting down server...\n");

            uint64_t u;
            if (read(server->shutdown_fd, &u, sizeof(u)) != sizeof(u)) {
                perror("read shutdown_fd failed");
            }
        
            server_shutdown(server);
        
            return;
          }
      }
  }

  printf("Server run loop exited.\n");
}

void server_shutdown(Server* server) {
  if (!server) return;

  printf("Shutting down server...\n");

  server->keep_running = 0;

  if (server->server_fd != -1) {
      close(server->server_fd);
      server->server_fd = -1;
  }

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

