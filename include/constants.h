// constants.h
// Constants for Argon Web Server

#ifndef CONSTANTS_H
#define CONSTANTS_H

// === Server Settings ===
#define SERVER_PORT 8080              // Default server port
#define BACKLOG 10                    // Maximum number of pending connections

// === Socket Options ===
#define ENABLE_SO_REUSEADDR 1         // Allow reuse of local addresses
#define ENABLE_SO_REUSEPORT 0         // (Optional) Enable multiple sockets on the same port

// === General States ===
#define ON 1
#define OFF 0

// === Logging Levels ===
#define LOG_LEVEL_DEBUG 0
#define LOG_LEVEL_INFO 1
#define LOG_LEVEL_ERROR 2

// Default log level
#define DEFAULT_LOG_LEVEL LOG_LEVEL_INFO

// === Buffer Sizes ===
#define BUFFER_SIZE 4096              // General-purpose buffer size
#define MAX_REQUEST_SIZE 8192         // Maximum allowed HTTP request size

// === Defaults for HTTP Responses ===
#define HTTP_RESPONSE_200 "HTTP/1.1 200 OK\r\nContent-Length: %zu\r\n\r\n%s"
#define HTTP_RESPONSE_500 "HTTP/1.1 500 Internal Server Error\r\nContent-Length: %zu\r\n\r\n%s"
#define HTTP_DEFAULT_BODY "Hello from Argon!"

#endif // CONSTANTS_H
