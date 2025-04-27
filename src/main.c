#include "../include/server.h"

int main() {
  Server* server = server_init("0.0.0.0", 8080);
  server_run(server);
  return 0;
}
