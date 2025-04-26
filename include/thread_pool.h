// thread_pool.h
// Simple thread pool for Argon server

#ifndef THREAD_POOL_H
#define THREAD_POOL_H
#include <pthread.h>
#define MAX_QUEUE_SIZE 1024
#define MAX_THREADS 4

typedef struct {
    pthread_mutex_t queue_mutex;
    pthread_cond_t queue_cond;
    int *client_queue;     // queue dynamic array
    int queue_front;
    int queue_rear;
    int queue_count;
    int max_queue_size;    // Max queue size

    pthread_t *threads;    // threads
    int thread_count;      // Current threads count
    int max_threads;       // Max threads size

    volatile int keep_running; // Flag pool is running
} ThreadPool;

ThreadPool* create_thread_pool(int num_threads, int queue_size);
thhread_pool_add_task(ThreadPool* pool, int client_fd);
thread_pool_destroy(ThreadPool *pool);

#endif  // THREAD_POOL_H
