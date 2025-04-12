// thread_pool.h
// Simple thread pool for Argon server

#ifndef THREAD_POOL_H
#define THREAD_POOL_H

#include <pthread.h>

void thread_pool_init(int num_threads);
void thread_pool_add_task(int client_fd);
void thread_pool_destroy();

extern pthread_mutex_t queue_mutex;
extern pthread_cond_t queue_cond;
#endif  // THREAD_POOL_H
