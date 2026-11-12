#ifndef ANGELLY_BRIDGE_H
#define ANGELLY_BRIDGE_H

#include <stdbool.h>
#include <stdint.h>

// Implemented by the Rust application. Every function must be called on the main
// thread. Returned strings are owned by the caller and released with
// angelly_string_free.

// JSON snapshot of the windows, tabs and terminals.
char *angelly_automation_snapshot(void);

// Executes a JSON request and returns the JSON response.
char *angelly_automation_request(const char *json);

void angelly_string_free(char *string);

// The `macos-applescript` configuration switch.
bool angelly_applescript_enabled(void);

// The `macos-shortcuts` configuration: 0 ask, 1 allow, 2 deny.
int32_t angelly_shortcuts_policy(void);

// The remembered answer to the Shortcuts prompt: -1 never asked, 0 denied, 1 allowed.
int32_t angelly_shortcuts_remembered(void);

void angelly_shortcuts_remember(bool allowed);

#endif
