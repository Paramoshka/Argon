#include <signal.h>

#include "../include/server.h"

int main() {
  // Register signal handler
  signal(SIGINT, handle_signal);
  signal(SIGTERM, handle_signal);
  start_server();
  return 0;
}
