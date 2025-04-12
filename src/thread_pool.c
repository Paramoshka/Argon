// thread_pool.c
// Simple thread pool implementation

#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

#include "../include/http.h"
#include "../include/thread_pool.h"

#define MAX_QUEUE_SIZE 1024

static int client_queue[MAX_QUEUE_SIZE];
static int queue_front = 0;
static int queue_rear = 0;
static int queue_count = 0;

pthread_mutex_t queue_mutex = PTHREAD_MUTEX_INITIALIZER;
pthread_cond_t queue_cond = PTHREAD_COND_INITIALIZER;

static int keep_running = 1;

static void* worker_thread(void* arg) {
  while (keep_running) {
    pthread_mutex_lock(&queue_mutex);

    while (queue_count == 0 && keep_running) {
      pthread_cond_wait(&queue_cond, &queue_mutex);
    }

    if (queue_count == 0 && !keep_running) {
      pthread_mutex_unlock(&queue_mutex);
      break;
    }

    if (!keep_running) {
      pthread_mutex_unlock(&queue_mutex);
      break;
    }

    // Dequeue client_fd
    int client_fd = client_queue[queue_front];
    queue_front = (queue_front + 1) % MAX_QUEUE_SIZE;
    queue_count--;

    pthread_mutex_unlock(&queue_mutex);

    // Process the client
    handle_client(client_fd);
  }

  return NULL;
}

void thread_pool_init(int num_threads) {
  pthread_t thread;

  for (int i = 0; i < num_threads; ++i) {
    if (pthread_create(&thread, NULL, worker_thread, NULL) != 0) {
      perror("pthread_create failed");
      exit(EXIT_FAILURE);
    }
    pthread_detach(thread);  // Detach thread, we don't need to join
  }

  printf("Thread pool initialized with %d threads.\n", num_threads);
}

void thread_pool_add_task(int client_fd) {
  pthread_mutex_lock(&queue_mutex);

  if (queue_count < MAX_QUEUE_SIZE) {
    client_queue[queue_rear] = client_fd;
    queue_rear = (queue_rear + 1) % MAX_QUEUE_SIZE;
    queue_count++;

    pthread_cond_signal(&queue_cond);
  } else {
    fprintf(stderr, "Task queue full, rejecting connection.\n");
    close(client_fd);
  }

  pthread_mutex_unlock(&queue_mutex);
}

void thread_pool_destroy() {
  pthread_mutex_lock(&queue_mutex);
  keep_running = 0;
  pthread_cond_broadcast(&queue_cond);
  pthread_mutex_unlock(&queue_mutex);
}
