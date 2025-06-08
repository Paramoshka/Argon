#ifndef REQUEST_H
#define REQUEST_H

#define MAX_HEADERS 32
#define MAX_HEADER_NAME 64
#define MAX_HEADER_VALUE 256
#define MAX_PATH 256
#define MAX_QUERY 256

typedef struct {
    char name[MAX_HEADER_NAME];
    char value[MAX_HEADER_VALUE];
} Header;

typedef struct {
    char method[8];
    char path[MAX_PATH];
    char query[MAX_QUERY];
    char version[16];

    Header headers[MAX_HEADERS];
    int header_count;
} Request;

Request* parse_http_request(int client_fd);
void free_request(Request* req);
const char* get_header(const Request* req, const char* key);

#endif // REQUEST_H