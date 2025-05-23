#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <arpa/inet.h>
#include "../include/http.h"
#include "../include/request.h"

void handle_client(int client_fd, ServerConfig* config) {
    struct sockaddr_in local_addr;
    socklen_t addrlen = sizeof(local_addr);
    if (getsockname(client_fd, (struct sockaddr*)&local_addr, &addrlen) == 0) {
        int local_port = ntohs(local_addr.sin_port);

        Request* req = parse_http_request(client_fd);
        if (!req) {
            close(client_fd);
            return;
        }
        const char* host = get_header(req, "Host");
        free_request(req);
        ServerName* selected = NULL;
        ServerName *s, *tmp;
        HASH_ITER(hh, config->servers, s, tmp) {
            if (s->listen == local_port) {
                if (host) {
                    for (int k = 0; k < s->server_name_count; k++) {
                        if (strcmp(s->server_names[k], host) == 0) {
                            selected = s;
                            break;
                        }
                    }
                }
                if (!selected) {
                    selected = s;  // Default to first matching port
                }
                if (selected) break;
            }
        }

        if (selected) {
            // Use selected->root, selected->locations, etc.
            char response[1024];
            snprintf(response, sizeof(response),
                     "HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\nHello, World!");
            write(client_fd, response, strlen(response));
        } else {
            char response[1024];
            snprintf(response, sizeof(response),
                     "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot Found");
            write(client_fd, response, strlen(response));
        }
    }

    close(client_fd);
}
