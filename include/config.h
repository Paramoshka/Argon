#ifndef CONFIG_H
#define CONFIG_H

#include <stdio.h>          // FILE*
#include <stdlib.h>         // malloc, calloc, free
#include <string.h>         // strcpy, strncpy, strcmp
#include <stdint.h>         // fixed width types
#include "../vendor/uthash/include/uthash.h"  // uthash for server map

// Maximum limits
#define MAX_SERVER_NAMES 8
#define MAX_RULES 16
#define MAX_LOCATIONS 16
#define MAX_LINE 1024
#define MAX_SERVERS 64

// Structure for location block
typedef struct {
    char path[128];         // e.g., "/static"
    char root[128];         // e.g., "/var/www/static"
    char autoindex[8];      // "on" or "off"
} Location;

// Structure for server block
typedef struct {
    char server_names[MAX_SERVER_NAMES][128];   // server_name entries
    int server_name_count;

    int listen;           // listen port
    int ratelimit;        // rate limit per second

    char allow[MAX_RULES][32];   // allowed CIDRs
    int allow_count;

    char deny[MAX_RULES][32];    // denied CIDRs
    int deny_count;

    Location locations[MAX_LOCATIONS];
    int location_count;

    // uthash fields
    char key[128];               // used as hash key: first server_name
    UT_hash_handle hh;
} ServerName;

// Global config containing all servers
typedef struct {
    ServerName *servers;   // hash map by key (server_name[0])
    int server_count;
} ServerConfig;

// Parses the given config file and fills the ServerConfig structure.
// Returns 0 on success, -1 on failure.
int parse_config(const char *config_path, ServerConfig *config_out);

// Print all config for debug
void print_config(ServerConfig *config);

#endif  // CONFIG_H
