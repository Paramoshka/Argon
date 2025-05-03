#include "../include/server.h"
#include "../include/http.h"
#include "../include/config.h"



const char * cfg_path = "argon.conf";

int main() {
  ServerConfig* cfg = calloc(1, sizeof(ServerConfig));
  if (parse_config(cfg_path, cfg) == -1)
  {
    perror("Read conf failed");
    return 1;
  }
  print_config(cfg);
  
  Server* server = server_init(cfg, handle_client);
  server_run(server);
  server_shutdown(server);
  return 0;
}
