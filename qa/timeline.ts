// Development-only visual fixture. Not an input to the production Vite build.
import { mount } from 'svelte';
import { api } from '../src/lib/api';
import type { EventKind, Item } from '../src/lib/types';
import DashboardView from '../src/views/DashboardView.svelte';
import '../src/styles.css';

if (!import.meta.env.DEV || location.hostname !== '127.0.0.1') throw new Error('Use the isolated 127.0.0.1 development origin.');
const storageKey = 'shanji.preview.items';
// Never overwrite existing browser preview content; use a fresh origin/session.
if (localStorage.getItem(storageKey) && !sessionStorage.getItem('shanji.qa.fixture')) throw new Error('This preview origin already has content.');
const template = (await api.listItems('all'))[0];
const iso = (day: number, hour = 9) => new Date(2026, 8, day, hour).toISOString();
const records: [string, EventKind, number, number][] = [
  ['核对第三季度项目预算', 'ORDINARY', 1, 12], ['准备周三评审会议材料', 'ONE_TIME', 4, 9],
  ['下班前提交发布审批', 'TODAY_MUST', 7, 7], ['秋季展会筹备', 'CONTINUOUS', 2, 24],
  ['服务器证书到期预警', 'WARNING', 1, 20], ['月度对账与发票整理', 'MONTHLY', 1, 10],
  ['年度设备维护', 'YEARLY', 1, 30], ['跟进供应商报价', 'ORDINARY', 3, 6],
  ['整理设计评审反馈', 'ONE_TIME', 5, 11], ['补齐客户交付清单', 'ORDINARY', 6, 16],
  ['预约项目复盘会议', 'ONE_TIME', 7, 15], ['归档上月验收材料', 'ONE_TIME', 1, 5],
];
const items: Item[] = records.map(([title, eventKind, start, end], index) => ({
  ...template, id: `qa-${index}`, title, notes: '', eventKind, createdAt: iso(start), updatedAt: iso(start), dueAt: iso(end, 18),
  dueLocalDate: `2026-09-${String(end).padStart(2, '0')}`, dueLocalTime: '18:00', status: index === 11 ? 'DONE' : 'OPEN',
  completedAt: index === 11 ? iso(end, 17) : null, important: index === 2, completionPolicy: index === 2 ? 'MUST_COMPLETE_TODAY' : 'NORMAL',
  categoryName: '工作', categoryId: 'work', tags: [], nextReminderAt: iso(end, 18),
  startAt: eventKind === 'CONTINUOUS' ? iso(10) : null, endAt: eventKind === 'CONTINUOUS' ? iso(end, 18) : null,
  targetAt: eventKind === 'WARNING' ? iso(end, 18) : null, leadValue: eventKind === 'WARNING' ? 1 : null, leadUnit: eventKind === 'WARNING' ? 'WEEK' : null,
}));
sessionStorage.setItem('shanji.qa.fixture', '1');
localStorage.setItem(storageKey, JSON.stringify(items));
localStorage.setItem('shanji.preview.onboarding-version', '3');

let failNext = false;
const read = api.getTimeline.bind(api);
api.getTimeline = async (query) => {
  if (failNext) { failNext = false; throw new Error('Synthetic read failure'); }
  return read(query);
};
mount(DashboardView, { target: document.getElementById('app')! });

const controls = document.createElement('div');
controls.style.cssText = 'position:fixed;bottom:0;left:0;right:0;z-index:9999;padding:4px 10px;background:#fff;color:#223349;border-top:1px solid #8799af;font:11px sans-serif;display:flex;gap:8px;align-items:center';
controls.innerHTML = '<span>开发验收 · 示例数据</span><select aria-label="验收主题"><option value="light">浅色</option><option value="dark">深色</option><option value="contrast">高对比度</option><option value="forced">强制颜色样式</option><option value="reduced">减少动态效果</option></select><button type="button">下次读取失败</button><button type="button">2000 条示例</button><button type="button">恢复示例</button>';
document.body.append(controls);
const mediaRules: { rule: CSSMediaRule; original: string }[] = [];
function collect(rules: CSSRuleList) {
  for (const rule of rules) {
    if (rule instanceof CSSMediaRule && /prefers-|forced-colors/.test(rule.conditionText) && !mediaRules.some((entry) => entry.rule === rule)) mediaRules.push({ rule, original: rule.conditionText });
    else if ('cssRules' in rule) collect((rule as CSSGroupingRule).cssRules);
  }
}
for (const sheet of document.styleSheets) collect(sheet.cssRules);
// Activate the application's actual media rules, without modifying OS/browser preferences.
controls.querySelector('select')!.addEventListener('change', (event) => {
  for (const sheet of document.styleSheets) collect(sheet.cssRules);
  const mode = (event.target as HTMLSelectElement).value;
  for (const { rule, original } of mediaRules) {
    const active = mode === 'dark' ? original.includes('prefers-color-scheme: dark') || original.includes('prefers-color-scheme:dark')
      : mode === 'contrast' ? original.includes('prefers-contrast') : mode === 'forced' ? original.includes('forced-colors')
      : mode === 'reduced' ? original.includes('prefers-reduced-motion') : false;
    rule.media.mediaText = active ? 'all' : 'not all';
  }
});
const buttons = controls.querySelectorAll('button');
buttons[0].onclick = () => { failNext = true; };
buttons[1].onclick = () => { localStorage.setItem(storageKey, JSON.stringify(Array.from({ length: 2000 }, (_, index) => ({ ...items[index % 11], id: `load-${index}`, title: `项目跟进 ${index + 1}`, eventKind: 'ORDINARY' })))); window.dispatchEvent(new Event('focus')); };
buttons[2].onclick = () => { localStorage.setItem(storageKey, JSON.stringify(items)); window.dispatchEvent(new Event('focus')); };
