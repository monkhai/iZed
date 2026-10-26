#include <stdbool.h>
#include <stdint.h>
void terminal_add_host(const char *name, const char *address, const char *command);
void terminal_refresh_machines(void);
void terminal_update_tailnet(const char *status_json, const char *proxy_host, uint16_t proxy_port, const char *proxy_credential, const char *error);
