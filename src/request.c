#include "../include/request.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <ctype.h>

static void trim(char* str) {
    char* end;
    while (isspace((unsigned char)*str)) str++;
    end = str + strlen(str) - 1;
    while (end > str && isspace((unsigned char)*end)) *end-- = '\0';
}

const char* get_header(const Request* req, const char* key) {
    for (int i = 0; i < req->header_count; i++) {
        if (strcasecmp(req->headers[i].name, key) == 0) {
            return req->headers[i].value;
        }
    }
    return NULL;
}

Request* parse_http_request(int client_fd) {
    char buffer[4096];
    int total = 0;
    Request* req = calloc(1, sizeof(Request));

    while (1) {
        ssize_t n = read(client_fd, buffer + total, sizeof(buffer) - total - 1);
        if (n <= 0) {
            perror("read");
            free(req);
            return NULL;
        }
        total += n;
        buffer[total] = '\0';

        if (strstr(buffer, "\r\n\r\n")) break;
        if (total >= sizeof(buffer) - 1) {
            fprintf(stderr, "Header too large\n");
            free(req);
            return NULL;
        }
    }

    char* line = strtok(buffer, "\r\n");
    if (!line) return NULL;

    sscanf(line, "%7s %255s %15s", req->method, req->path, req->version);

    // Split query
    char* q = strchr(req->path, '?');
    if (q) {
        *q = '\0';
        strncpy(req->query, q + 1, MAX_QUERY - 1);
    }

    // Parse headers
    while ((line = strtok(NULL, "\r\n")) != NULL && req->header_count < MAX_HEADERS) {
        char* colon = strchr(line, ':');
        if (!colon) continue;
        *colon = '\0';
        strncpy(req->headers[req->header_count].name, line, MAX_HEADER_NAME - 1);
        strncpy(req->headers[req->header_count].value, colon + 1, MAX_HEADER_VALUE - 1);
        trim(req->headers[req->header_count].value);
        req->header_count++;
    }

    return req;
}

void free_request(Request* req) {
    if (req) free(req);
}
