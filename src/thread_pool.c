// thread_pool.c
// Simple thread pool implementation

#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

#include "../include/http.h"
#include "../include/server.h"
#include "../include/thread_pool.h"


ThreadPool* create_thread_pool(int num_threads, int queue_size) {
  ThreadPool* thread_pool = calloc(1, sizeof(ThreadPool));
  if (!thread_pool) {
    perror("Failed create thread pool in calloc");
    exit(EXIT_FAILURE);
  }

  if (pthread_mutex_init(&thread_pool->queue_mutex, NULL) != 0) {
    perror("Failed to initialize pool_thread mutex queue");
    free(thread_pool);
    exit(EXIT_FAILURE);
  }

  if (pthread_cond_init(&thread_pool->queue_cond, NULL) != 0) {
    perror("Failed to initialize queue condvar");
    pthread_mutex_destroy(&thread_pool->queue_mutex);
    free(thread_pool);
    exit(EXIT_FAILURE);
  }

  // Allocate memory for client
  thread_pool->client_queue = calloc(queue_size, sizeof(int));
  if (!thread_pool->client_queue) {
    perror("Failed to allocate client queue");
    pthread_mutex_destroy(&thread_pool->queue_mutex);
    pthread_cond_destroy(&thread_pool->queue_cond);
    free(thread_pool);
    exit(EXIT_FAILURE);
  }

  // Allocate memory for threads
  thread_pool->threads = calloc(num_threads, sizeof(pthread_t));
  if (!thread_pool->threads) {
    perror("Failed to allocate thread array");
    free(thread_pool->client_queue);
    pthread_mutex_destroy(&thread_pool->queue_mutex);
    pthread_cond_destroy(&thread_pool->queue_cond);
    free(thread_pool);
    exit(EXIT_FAILURE);
  }

  // set threaed pool parameters

  thread_pool->queue_front = 0;
  thread_pool->queue_rear = 0;
  thread_pool->queue_count = 0;
  thread_pool->max_queue_size = queue_size;
  thread_pool->thread_count = 0;
  thread_pool->max_threads = num_threads;
  thread_pool->keep_running = 1;


  return thread_pool;
}

// static int queue_front = 0;
// static int queue_rear = 0;
// static int queue_count = 0;
// static int thread_count = 0;
// static int client_queue[MAX_QUEUE_SIZE];
// static pthread_t threads[MAX_THREADS];

// pthread_mutex_t queue_mutex = PTHREAD_MUTEX_INITIALIZER;
// pthread_cond_t queue_cond = PTHREAD_COND_INITIALIZER;

// static void* worker_thread(void* arg) {
//   printf("Worker thread started.\n");

//   while (1) {
//     pthread_mutex_lock(&queue_mutex);

//     if (!keep_running && queue_count == 0) {
//       pthread_mutex_unlock(&queue_mutex);
//       printf("Worker thread exiting due to shutdown.\n");
//       break;
//     }

//     while (queue_count == 0 && keep_running) {
//       printf("Worker thread waiting for tasks...\n");
//       pthread_cond_wait(&queue_cond, &queue_mutex);
//     }

//     if (!keep_running && queue_count == 0) {
//       pthread_mutex_unlock(&queue_mutex);
//       printf("Worker thread exiting due to shutdown.\n");
//       break;
//     }

//     int client_fd = client_queue[queue_front];
//     queue_front = (queue_front + 1) % MAX_QUEUE_SIZE;
//     queue_count--;

//     pthread_mutex_unlock(&queue_mutex);

//     handle_client(client_fd);
//   }

//   return NULL;
// }

// void thread_pool_init(int num_threads) {
//   pthread_t thread;

//   for (int i = 0; i < num_threads; ++i) {
//     if (pthread_create(&thread, NULL, worker_thread, NULL) != 0) {
//       perror("pthread_create failed");
//       exit(EXIT_FAILURE);
//     }

//     threads[thread_count++] = thread;

//     printf("Worker thread %d started.\n", thread_count);
//   }

//   printf("Thread pool initialized with %d threads.\n", thread_count);
// }

// void thread_pool_add_task(int client_fd) {
//   pthread_mutex_lock(&queue_mutex);

//   if (queue_count < MAX_QUEUE_SIZE) {
//     client_queue[queue_rear] = client_fd;
//     queue_rear = (queue_rear + 1) % MAX_QUEUE_SIZE;
//     queue_count++;

//     pthread_cond_signal(&queue_cond);
//   } else {
//     fprintf(stderr, "Task queue full, rejecting connection.\n");
//     close(client_fd);
//   }

//   pthread_mutex_unlock(&queue_mutex);
// }

// void thread_pool_destroy() {

//   pthread_mutex_lock(&queue_mutex);
//   pthread_cond_broadcast(&queue_cond);
//   pthread_mutex_unlock(&queue_mutex);


//   for (int i = 0; i < thread_count; ++i) {
//     if (pthread_join(threads[i], NULL) != 0) {
//       perror("pthread_join failed");
//     } else {
//       printf("Worker thread %d exited.\n", i + 1);
//     }
//   }

//   pthread_mutex_destroy(&queue_mutex);
//   pthread_cond_destroy(&queue_cond);

//   printf("Thread pool destroyed.\n");
// }
