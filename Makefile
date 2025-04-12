CC = gcc
CFLAGS = -Wall -Wextra -Iinclude -static
SRC = $(wildcard src/*.c)
TARGET = argon

all:
	$(CC) $(CFLAGS) $(SRC) -o $(TARGET)

clean:
	rm -f $(TARGET)

