.PHONY: all build run clean

CC = clang

SRC_DIR = source
BUILD_DIR = build
TARGET = $(BUILD_DIR)/yarrow

CFLAGS = -Wall -Wextra -I$(SRC_DIR) -I.
LDFLAGS =

SOURCES = $(shell find $(SRC_DIR) -type f -name '*.c')
OBJECTS = $(patsubst $(SRC_DIR)/%.c,$(BUILD_DIR)/%.o,$(SOURCES))

all: build

$(TARGET): $(OBJECTS)
	$(CC) $(OBJECTS) -o $@ $(LDFLAGS)

$(BUILD_DIR)/%.o: $(SRC_DIR)/%.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -c $< -o $@

build: $(TARGET)

run: build
	./$(TARGET)

clean:
	rm -rf $(BUILD_DIR)
