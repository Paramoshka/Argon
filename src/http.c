// http.c
// HTTP request handling for Argon

#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include "../include/constants.h"

void handle_client(int client_fd) {
  char buffer[BUFFER_SIZE];
  ssize_t bytes_read;

  // Read client request
  bytes_read = read(client_fd, buffer, sizeof(buffer) - 1);
  if (bytes_read < 0) {
    perror("read failed");
    close(client_fd);
    return;
  }

  buffer[bytes_read] = '\0';  // Null-terminate the buffer

  printf("Received request:\n%s\n", buffer);

  // Prepare HTTP response
  const char *body = HTTP_DEFAULT_BODY;
  char response[BUFFER_SIZE];
  int response_length = snprintf(response, sizeof(response), HTTP_RESPONSE_200,
                                 strlen(body), body);

  // Send HTTP response
  if (write(client_fd, response, response_length) < 0) {
    perror("write failed");
  }

  // Close the connection
  close(client_fd);
  printf("Connection closed.\n");
}
