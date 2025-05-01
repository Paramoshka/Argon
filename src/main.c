#include "../include/server.h"
#include "../include/http.h"

int main() {
  Server* server = server_init("0.0.0.0", 8080, handle_client);
  server_run(server);
  server_shutdown(server);
  return 0;
}
