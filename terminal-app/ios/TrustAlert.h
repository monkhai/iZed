#import <UIKit/UIKit.h>

// Shared by the shipping app and the native catalog. Keep host verification UI
// in one factory; callers own the actual trust decision.
static inline UIAlertController *TerminalMakeTrustAlert(NSString *host, NSString *identity, BOOL changed, void (^completion)(BOOL)) {
    NSString *message = [NSString stringWithFormat:@"%@\n\n%@\n\nCompare this fingerprint with your host before trusting it.", host, identity];
    UIAlertController *alert = [UIAlertController alertControllerWithTitle:changed ? @"Host identity changed" : @"Verify host identity"
        message:message preferredStyle:UIAlertControllerStyleAlert];
    [alert addAction:[UIAlertAction actionWithTitle:@"Cancel" style:UIAlertActionStyleCancel handler:^(UIAlertAction *action) { completion(NO); }]];
    [alert addAction:[UIAlertAction actionWithTitle:changed ? @"Trust new identity" : @"Trust and connect"
        style:changed ? UIAlertActionStyleDestructive : UIAlertActionStyleDefault handler:^(UIAlertAction *action) { completion(YES); }]];
    alert.overrideUserInterfaceStyle = UIUserInterfaceStyleDark;
    return alert;
}
