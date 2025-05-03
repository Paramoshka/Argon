// http.h
// HTTP handling functions for Argon

#ifndef HTTP_H
#define HTTP_H
#include "../include/config.h"

void handle_client(int client_fd, ServerConfig* config);

#endif  // HTTP_H
