// Share iZed's scene lifecycle and GPUI bridge; keep this app's identity separate.
#include "../../app/ios/main.m"
#import <Security/Security.h>

extern void terminal_add_host(const char *, const char *, const char *);
extern void terminal_trust_host(bool);

static UIViewController *terminalController(void) {
    for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
        if (![scene isKindOfClass:UIWindowScene.class]) continue;
        for (UIWindow *window in ((UIWindowScene *)scene).windows) {
            if (!window.isKeyWindow) continue;
            UIViewController *controller = window.rootViewController;
            while (controller.presentedViewController) controller = controller.presentedViewController;
            return controller;
        }
    }
    return nil;
}

static void terminalPresentTrust(UIAlertController *alert, NSUInteger attempt) {
    UIViewController *controller = terminalController();
    if (UIApplication.sharedApplication.applicationState != UIApplicationStateActive ||
        !controller || !controller.view.window || controller.isBeingDismissed ||
        controller.transitionCoordinator) {
        if (attempt >= 20) {
            // Never strand the Rust connection state behind an invisible modal.
            terminal_trust_host(false);
            return;
        }
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 100 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
            terminalPresentTrust(alert, attempt + 1);
        });
        return;
    }
    [controller presentViewController:alert animated:YES completion:nil];
}

void terminal_show_trust(const char *address, const char *fingerprint, bool changed) {
    NSString *host = [NSString stringWithUTF8String:address];
    NSString *identity = [NSString stringWithUTF8String:fingerprint];
    dispatch_async(dispatch_get_main_queue(), ^{
        NSString *message = [NSString stringWithFormat:@"%@\n\n%@\n\nCompare this fingerprint with your host before trusting it.", host, identity];
        UIAlertController *alert = [UIAlertController alertControllerWithTitle:changed ? @"Host identity changed" : @"Verify host identity"
            message:message preferredStyle:UIAlertControllerStyleAlert];
        [alert addAction:[UIAlertAction actionWithTitle:@"Cancel" style:UIAlertActionStyleCancel handler:^(UIAlertAction *action) { terminal_trust_host(false); }]];
        [alert addAction:[UIAlertAction actionWithTitle:changed ? @"Trust new identity" : @"Trust and connect"
            style:changed ? UIAlertActionStyleDestructive : UIAlertActionStyleDefault handler:^(UIAlertAction *action) { terminal_trust_host(true); }]];
        alert.overrideUserInterfaceStyle = UIUserInterfaceStyleDark;
        terminalPresentTrust(alert, 0);
    });
}

static NSDictionary *keyQuery(void) {
    return @{(__bridge id)kSecClass: (__bridge id)kSecClassGenericPassword,
        (__bridge id)kSecAttrService: @"com.monkhai.ized-terminal.ssh",
        (__bridge id)kSecAttrAccount: @"device-ed25519"};
}

// Returned memory is cleared and freed by Rust. Key material never enters logs.
char *terminal_load_key(void) {
    NSMutableDictionary *query = [keyQuery() mutableCopy];
    query[(__bridge id)kSecReturnData] = @YES;
    CFTypeRef result = NULL;
    if (SecItemCopyMatching((__bridge CFDictionaryRef)query, &result) != errSecSuccess) return NULL;
    NSData *data = CFBridgingRelease(result);
    char *buffer = calloc(data.length + 1, 1);
    memcpy(buffer, data.bytes, data.length);
    return buffer;
}

int32_t terminal_store_key(const char *key) {
    NSMutableDictionary *query = [keyQuery() mutableCopy];
    query[(__bridge id)kSecValueData] = [[NSString stringWithUTF8String:key] dataUsingEncoding:NSUTF8StringEncoding];
    query[(__bridge id)kSecAttrAccessible] = (__bridge id)kSecAttrAccessibleWhenUnlockedThisDeviceOnly;
    return SecItemAdd((__bridge CFDictionaryRef)query, NULL);
}

void terminal_free_key(char *key) {
    if (key) { memset_s(key, strlen(key), 0, strlen(key)); free(key); }
}

// Give VoiceOver and UI tests the actual visible terminal grid, not placeholder text.
void terminal_accessibility(const char *value, double x, double y, double width, double height) {
    static UIView *accessibleTerminal;
    if (!accessibleTerminal) {
        accessibleTerminal = [[UIView alloc] initWithFrame:CGRectZero];
        accessibleTerminal.isAccessibilityElement = YES;
        accessibleTerminal.accessibilityIdentifier = @"ssh-terminal-screen";
        accessibleTerminal.accessibilityLabel = @"SSH terminal";
        accessibleTerminal.accessibilityTraits = UIAccessibilityTraitStaticText;
        accessibleTerminal.userInteractionEnabled = NO;
        [terminalController().view addSubview:accessibleTerminal];
    }
    accessibleTerminal.frame = CGRectMake(x, y, width, height);
    NSString *text = [NSString stringWithUTF8String:value];
    if (![accessibleTerminal.accessibilityValue isEqualToString:text]) accessibleTerminal.accessibilityValue = text;
}
