#import "CatalogBridge.h"
extern void catalog_refresh_fixture(void);
extern void terminal_trust_host(bool);
void terminal_start_tailscale(void) {}
void terminal_refresh_tailscale(void) { dispatch_async(dispatch_get_main_queue(), ^{catalog_refresh_fixture();}); }
char *terminal_load_key(void) { return NULL; }
int32_t terminal_store_key(const char *key) { return -1; }
void terminal_free_key(char *key) {}
void terminal_accessibility(const char *value,double x,double y,double width,double height) {}
void terminal_show_trust(const char *address,const char *fingerprint,bool changed) {
    dispatch_async(dispatch_get_main_queue(), ^{
        for(UIWindowScene *scene in UIApplication.sharedApplication.connectedScenes) {
            if (![scene isKindOfClass:UIWindowScene.class]) continue;
            for(UIWindow *window in scene.windows) if(window.isKeyWindow) {
                UIViewController *vc=window.rootViewController;
                while(vc.presentedViewController) vc=vc.presentedViewController;
                [vc presentViewController:TerminalMakeTrustAlert(@(address),@(fingerprint),changed,^(BOOL accepted){terminal_trust_host(accepted);}) animated:NO completion:nil];
                return;
            }
        }
    });
}
