CC = gcc
CFLAGS = -Wall -Wextra -Iinclude
SRC = src/main.c src/server.c src/http.c
TARGET = argon

all:
	$(CC) $(CFLAGS) $(SRC) -o $(TARGET)

clean:
	rm -f $(TARGET)

