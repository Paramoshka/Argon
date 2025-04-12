argon/
├── src/                  # Source
│   ├── main.c            # Entry point
│   ├── server.c          # serve cycle (accept, listen, epoll и т.п.)
│   ├── http.c            # parse and handle HTTP
│   ├── config.c          # Config (TOML/YAML/ini)
│   ├── modules/          # (dlopen or static)
│   │   ├── static_file.c # simple module for response static files
│   │   └── reverse_proxy.c # Reverse proxy
│   └── utils.c           # 
├── include/              # Header files
│   ├── server.h
│   ├── http.h
│   └── config.h
├── tests/                # (CTest)
│   └── test_http.c
├── examples/             # Samples
│   └── example.conf
├── scripts/              # Scripts for build and run
│   └── build.sh
├── Makefile              # 
├── README.md             # 
├── LICENSE               # (MIT)
└── CONTRIBUTING.md       #

