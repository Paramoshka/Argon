#include <asm-generic/socket.h>
#include <netinet/in.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <arpa/inet.h>
#include <errno.h>
#include <sys/socket.h>
#include "../include/constants.h"


#define SERVER_PORT 8080
#define BACKLOG 10
#define ENABLE_SO_REUSEADDR 1


void fatal(const char *message) {
    perror(message);
    exit(EXIT_FAILURE);
}

int main() {
    int server_fd;
    struct sockaddr_in server_addr;

    server_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (server_fd == -1) {
        fatal("[Argon] Can't create socket");
    }
    int opt = 1;
    setsockopt(server_fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));

    // Step 3: Prepare the sockaddr_in structure
    memset(&server_addr, 0, sizeof(server_addr)); // Clear structure
    server_addr.sin_family = AF_INET;
    server_addr.sin_addr.s_addr = INADDR_ANY;    // Listen on all interfaces
    server_addr.sin_port = htons(SERVER_PORT);   // Convert port to network byte order

    // Step 4: Bind the socket
    if (bind(server_fd, (struct sockaddr *)&server_addr, sizeof(server_addr)) == -1) {
        fatal("bind failed");
    }

        printf("Socket successfully bound to port %d\n", SERVER_PORT);

    // Step 5: Listen for incoming connections
    if (listen(server_fd, BACKLOG) == -1) {
        fatal("listen failed");
    }

    printf("Listening for incoming connections...\n");

    // Placeholder: Accept loop will be added later
    while (1) {
        pause(); // Temporarily wait for signals (replace with accept loop later)
    }

    // Step 6: Cleanup (unreachable for now)
    close(server_fd);

    return 0;

}

