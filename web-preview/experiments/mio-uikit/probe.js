// MIO calls the application entry point on load; this diagnostic has no app lifecycle.
function main() {}
/* Runtime capability checks taken from terminal-app/ios/MachinePicker.swift.
 * This is a diagnostic, not a translation of that Swift screen. */
const results = [];
function check(api, usage, fn) {
  try { fn(); results.push({api, usage, pass:true, detail:'Available'}); }
  catch(e) { results.push({api, usage, pass:false, detail:e.message}); }
}
function requireValue(value, api) { if (value === undefined || value === null) throw Error(api + ' is missing'); return value; }
check('UILabel', 'Search note and field labels', () => {
  const label = new UILabel(); label.init(); label.text = 'Actual MIO UILabel — runtime loaded';
  document.querySelector('#control').appendChild(label.layer);
});
check('UITableViewController', 'MachinePicker superclass', () => requireValue(window.UITableViewController,'UITableViewController'));
check('UINavigationBarAppearance', 'Picker viewDidLoad()', () => requireValue(window.UINavigationBarAppearance,'UINavigationBarAppearance'));
check('UIView.leadingAnchor', 'Search and footer constraints', () => {const view=new UIView();view.init();requireValue(view.leadingAnchor,'leadingAnchor');});
check('NSLayoutConstraint.activate', 'Picker and editor layout', () => requireValue(window.NSLayoutConstraint?.activate,'NSLayoutConstraint.activate'));
check('UIStackView', 'ConnectionEditor viewDidLoad()', () => requireValue(window.UIStackView,'UIStackView'));
check('UIView.safeAreaLayoutGuide', 'Connection editor scroll bounds', () => {const view=new UIView();view.init();requireValue(view.safeAreaLayoutGuide,'safeAreaLayoutGuide');});
check('UIView.keyboardLayoutGuide', 'Connection editor keyboard bounds', () => {const view=new UIView();view.init();requireValue(view.keyboardLayoutGuide,'keyboardLayoutGuide');});
check('UIButton.addAction / UIAction', 'Launch command and copy key actions', () => {requireValue(window.UIAction,'UIAction');requireValue(UIButton.prototype.addAction,'UIButton.addAction');});
check('UIImage(systemName:)', 'Refresh/search/device icons', () => requireValue(window.UIImage?.imageWithSystemName,'systemName initializer'));
for(const result of results) {
  const row=document.querySelector('#results').insertRow();
  row.insertCell().textContent=result.api;
  const status=row.insertCell();status.textContent=result.pass?'PASS':result.detail;status.className=result.pass?'pass':'fail';
  row.insertCell().textContent=result.usage;
}
const failed=results.filter(r=>!r.pass).length;
document.querySelector('#verdict').textContent=`${results.length-failed}/${results.length} runtime checks passed. ${failed} required APIs unavailable. The actual screen cannot run unchanged through this runtime.`;
