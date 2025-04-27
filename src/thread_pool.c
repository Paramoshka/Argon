// thread_pool.c
// Simple thread pool implementation

#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

#include "../include/http.h"
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

int thread_pool_add_task(ThreadPool* pool, int client_fd) {

  pthread_mutex_lock(&pool->queue_mutex);

  if (pool->queue_count == pool->max_queue_size) {
    perror("Queue is overflow");
    pthread_mutex_unlock(&pool->queue_mutex);
    return EXIT_FAILURE;
  }

  // Ring buffer
  pool->client_queue[pool->queue_rear] = client_fd;
  pool->queue_rear = (pool->queue_rear + 1) % pool->max_queue_size;
  pool->queue_count++;

  pthread_cond_signal(&pool->queue_cond);
  pthread_mutex_unlock(&pool->queue_mutex);

  return EXIT_SUCCESS;
}


void thread_pool_destroy(ThreadPool *pool) {

  if (!pool) return;

  pthread_mutex_lock(&pool->queue_mutex);
  pool->keep_running = 0;
  pthread_cond_broadcast(&pool->queue_cond);
  pthread_mutex_unlock(&pool->queue_mutex);

  // Waiit shutdown of all thrads
  for (size_t i = 0; i < pool->thread_count; i++)
  {
    if (pthread_join(pool->threads[i], NULL) != 0) {
      perror("Thread join is failed");
    }
  }

  pthread_mutex_destroy(&pool->queue_mutex);
  pthread_cond_destroy(&pool->queue_cond);

  free(pool->client_queue);
  free(pool->threads);
  free(pool);

  printf("Thread pool destroyed successfully.\n");
}

void* worker_thread(void* arg) {
  ThreadPool* pool = (ThreadPool*)arg;

  while (1)
  {
    pthread_mutex_lock(&pool->queue_mutex);

    while (pool->keep_running && pool->queue_count == 0) {
      pthread_cond_wait(&pool->queue_cond, &pool->queue_mutex);
    }

    if (!pool->keep_running && pool->queue_count == 0){
      pthread_mutex_unlock(&pool->queue_mutex);
      break;
    }

    int client_fd = pool->client_queue[pool->queue_front];
    pool->queue_front = (pool->queue_front + 1) % pool->max_queue_size;
    pool->queue_count--;

    pthread_mutex_unlock(&pool->queue_mutex);

    pool->task_handler(client_fd);
  }

  return NULL;
}


void thread_pool_start(ThreadPool* pool, void (*task_handler)(int)) {
  if (!pool || !task_handler) return;
  pool->task_handler = task_handler;

  for (int i = 0; i < pool->max_threads; i++) {
      if (pthread_create(&pool->threads[i], NULL, worker_thread, pool)) {
          perror("pthread_create err!");
          exit(EXIT_FAILURE);
      }
      pool->thread_count++;
  }

  printf("Thread pool started with %d threads.\n", pool->thread_count);
}


