#import "SurfaceProducer.h"
#import "../../../terminal-app/ios/TrustAlert.h"
#import "../../../app/ios/gpui_ios.h"
void gpui_ios_register_app(void);
void gpui_ios_run_app(void);
void terminal_catalog_select(const char *id);
#import "../../../terminal-app/ios/TerminalBridge.h"
char *terminal_catalog_snapshot(void);
void terminal_catalog_free(char *value);
