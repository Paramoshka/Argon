#include <ctype.h>
#include "../include/config.h"


// Trim space in begin and end string
void trim(char *str) {
    int start = 0, end = strlen(str) - 1;
    while (isspace(str[start])) start++;
    while (end >= 0 && (isspace(str[end]) || str[end] == ';')) end--;
    str[end + 1] = '\0';
    memmove(str, str + start, end - start + 2);
}

// Check exists string is number
int is_number(const char *str) {
    for (int i = 0; str[i]; i++) {
        if (!isdigit(str[i])) return 0;
    }
    return 1;
}

int parse_config(const char *config_path, ServerConfig *config_out) {
    FILE* file = fopen(config_path, "r");
    if (!file) {
        perror("Failed to open config");
        return -1;
    }

    char line[MAX_LINE];
    int in_server = 0, in_location = -1;
    ServerName *current_server = NULL;
    int brace_count = 0;
    int line_number = 0;

    config_out->servers = NULL;
    config_out->server_count = 0;

    while (fgets(line, sizeof(line), file)) {
        line_number++;
        trim(line);
        if (strlen(line) == 0) continue;

        // Count braces
        if (strstr(line, "{")) brace_count++;
        if (strstr(line, "}")) brace_count--;

        // Check context
        if (strstr(line, "server {")) {
            if (in_server) {
                fprintf(stderr, "Error: Nested server block at line %d\n", line_number);
                return -1;
            }
            if (config_out->server_count >= MAX_SERVERS) {
                fprintf(stderr, "Error: Too many servers at line %d\n", line_number);
                return -1;
            }
            in_server = 1;
            current_server = calloc(1, sizeof(ServerName));
            config_out->server_count++;
            continue;
        }

        if (in_server && strstr(line, "}")) {
            if (in_location >= 0) {
                in_location = -1; // Close location
            } else {
                if (current_server->server_name_count == 0) {
                    fprintf(stderr, "Error: No server_name specified at line %d\n", line_number);
                    return -1;
                }
                // Add serverName to hashMap
                strcpy(current_server->key, current_server->server_names[0]);
                ServerName *existing;
                HASH_FIND_STR(config_out->servers, current_server->key, existing);
                if (existing) {
                    fprintf(stderr, "Error: Duplicate server_name %s at line %d\n", current_server->key, line_number);
                    free(current_server);
                    return -1;
                }
                HASH_ADD_STR(config_out->servers, key, current_server);
                in_server = 0;
                current_server = NULL;
            }
            continue;
        }

        // Parse directive
        if (in_server && current_server) {
            if (strncmp(line, "server_name", 11) == 0) {
                char names[512];
                sscanf(line, "server_name %[^\n]", names);
                char *token = strtok(names, " ");
                while (token && current_server->server_name_count < MAX_SERVER_NAMES) {
                    strncpy(current_server->server_names[current_server->server_name_count++], token, 127);
                    token = strtok(NULL, " ");
                }
                if (token) {
                    fprintf(stderr, "Warning: Too many server_names at line %d\n", line_number);
                }
            } else if (strncmp(line, "listen", 6) == 0) {
                char port[32];
                sscanf(line, "listen %s", port);
                if (!is_number(port)) {
                    fprintf(stderr, "Error: Invalid listen port %s at line %d\n", port, line_number);
                    return -1;
                }
                current_server->listen = atoi(port);
            } else if (strncmp(line, "ratelimit", 9) == 0) {
                char rate[32];
                sscanf(line, "ratelimit %s", rate);
                if (!is_number(rate)) {
                    fprintf(stderr, "Error: Invalid ratelimit %s at line %d\n", rate, line_number);
                    return -1;
                }
                current_server->ratelimit = atoi(rate);
            } else if (strncmp(line, "allow", 5) == 0 && current_server->allow_count < MAX_RULES) {
                sscanf(line, "allow %s", current_server->allow[current_server->allow_count++]);
            } else if (strncmp(line, "deny", 4) == 0 && current_server->deny_count < MAX_RULES) {
                sscanf(line, "deny %s", current_server->deny[current_server->deny_count++]);
            } else if (strncmp(line, "location", 8) == 0 && current_server->location_count < MAX_LOCATIONS) {
                in_location = current_server->location_count++;
                sscanf(line, "location %s {", current_server->locations[in_location].path);
            }
        }

        // Parse location
        if (in_location >= 0 && current_server) {
            if (strncmp(line, "root", 4) == 0) {
                sscanf(line, "root %s", current_server->locations[in_location].root);
            } else if (strncmp(line, "autoindex", 9) == 0) {
                char autoindex[8];
                sscanf(line, "autoindex %s", autoindex);
                if (strcmp(autoindex, "on") != 0 && strcmp(autoindex, "off") != 0) {
                    fprintf(stderr, "Error: Invalid autoindex value %s at line %d\n", autoindex, line_number);
                    return -1;
                }
                strcpy(current_server->locations[in_location].autoindex, autoindex);
            }
        }
    }

    fclose(file);

    if (brace_count != 0) {
        fprintf(stderr, "Error: Unmatched braces in configuration\n");
        return -1;
    }
    if (in_server) {
        fprintf(stderr, "Error: Unclosed server block\n");
        free(current_server);
        return -1;
    }

    return 0;
}

// find server by server_name server_name
ServerName *find_server(ServerConfig *config, const char *server_name) {
    ServerName *server;
    HASH_FIND_STR(config->servers, server_name, server);
    return server;
}

// Result
void print_config(ServerConfig *config) {
    ServerName *server, *tmp;
    HASH_ITER(hh, config->servers, server, tmp) {
        printf("Server:\n");
        printf("  server_names:\n");
        for (int i = 0; i < server->server_name_count; i++) {
            printf("    - %s\n", server->server_names[i]);
        }
        printf("  listen: %d\n", server->listen);
        printf("  ratelimit: %d\n", server->ratelimit);
        printf("  allow:\n");
        for (int i = 0; i < server->allow_count; i++) {
            printf("    - %s\n", server->allow[i]);
        }
        printf("  deny:\n");
        for (int i = 0; i < server->deny_count; i++) {
            printf("    - %s\n", server->deny[i]);
        }
        printf("  locations:\n");
        for (int i = 0; i < server->location_count; i++) {
            printf("    - path: %s\n", server->locations[i].path);
            printf("      root: %s\n", server->locations[i].root);
            printf("      autoindex: %s\n", server->locations[i].autoindex);
        }
    }
}

// Free mem
void free_config(ServerConfig *config) {
    ServerName *server, *tmp;
    HASH_ITER(hh, config->servers, server, tmp) {
        HASH_DEL(config->servers, server);
        free(server);
    }
}