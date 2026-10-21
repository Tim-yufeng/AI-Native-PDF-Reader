// A self-contained layout preview: no PDF/AI imports, storage, network or file access.
const layout = document.body.dataset.layout === 'b' ? 'b' : 'a';
const paths = {
  file: '<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v5h4M9 12h6M9 16h6"/>',
  folder: '<path d="M3 7h7l2 2h9v11H3z"/><path d="M3 7V4h6l2 3"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  close: '<path d="m6 6 12 12M18 6 6 18"/>',
  thumbs: '<rect x="4" y="3" width="16" height="18" rx="2"/><path d="M9 3v18M4 9h5M4 15h5"/>',
  search: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 4.5 4.5"/>',
  previous: '<path d="m14 6-6 6 6 6"/>',
  next: '<path d="m10 6 6 6-6 6"/>',
  down: '<path d="m7 10 5 5 5-5"/>',
  minus: '<path d="M5 12h14"/>',
  more: '<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>',
  ai: '<path d="m12 3 2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5z"/>',
  pen: '<path d="m5 16 10-10 3 3L8 19l-4 1zM13 8l3 3M5 16l3 3"/>',
  region: '<path d="M8 3H3v5M16 3h5v5M21 16v5h-5M8 21H3v-5"/><rect x="7" y="7" width="10" height="10" rx="1" stroke-dasharray="2 3"/>',
  width: '<path d="M4 5v14M20 5v14M4 12h16m-12-4-4 4 4 4m8-8 4 4-4 4"/>',
  fit: '<rect x="6" y="3" width="12" height="18" rx="1"/><path d="M9 8h6M9 12h6M9 16h4"/>',
  history: '<path d="M4 10a8 8 0 1 1 0 6M4 5v5h5M12 7v5l3 2"/>',
  send: '<path d="m4 11 16-7-7 16-2-7zM11 13l9-9"/>',
};
const icon = (name) => `<svg class="icon" aria-hidden="true" viewBox="0 0 24 24">${paths[name] || paths.file}</svg>`;
const button = (name, label, action, extra = '') => `<button class="icon-button" type="button" aria-label="${label}" title="${label}" data-action="${action}" ${extra}>${icon(name)}</button>`;
const navigation = () => `<div class="reading-controls" aria-label="阅读导航"><div class="page-controls">${button('previous', '上一页', 'previous')}<input class="page-input" aria-label="页码，按回车跳转" type="number" min="1" max="8" value="1"><span class="page-total">/ 8</span>${button('next', '下一页', 'next')}</div><span class="separator"></span><div class="zoom-controls">${button('minus', '缩小', 'zoom-out')}<button class="zoom-trigger" title="缩放与适合方式" aria-label="缩放与适合方式" aria-expanded="false" data-action="zoom-menu"><span class="zoom-label">100%</span>${icon('down')}</button>${button('plus', '放大', 'zoom-in')}</div></div>`;
const titles = ['函数与变化率', '用局部变化理解导数', '从斜率到线性近似', '连续与可微', '复合函数的变化', '导数的几何解释', '例题与思考', '总结：把公式放回图像'];
const pageMarkup = (number) => `<article class="paper" data-page="${number}" aria-label="示意文档第 ${number} 页"><div class="paper-header"><span>阅读器设计示例 · 自制内容</span><span>CHAPTER 02</span></div><h1>${titles[number - 1]}</h1><p class="abstract">从曲线的局部变化，理解数学表达背后的几何意义。</p><h2>${number}.1 从一个问题出发</h2><p>如果只观察函数在很小一段范围内的变化，我们能否用一条直线描述它？<span class="searchable">变化率</span>把这个直觉变成了可以计算的量。</p><p>让增量逐渐接近零，割线的斜率趋于一个稳定值。这个值告诉我们：在当前点附近，输入变化一点，输出会怎样变化。</p><div class="equation" ${number === 1 ? 'id="region-target"' : ''}>f′(x) = lim<sub>h → 0</sub> <span>( f(x + h) − f(x) ) / h</span></div><h2>${number}.2 看见公式的含义</h2><p>导数不仅是一个运算结果，也是一种局部近似。把相邻的点联系起来，就可以同时理解切线、<span class="searchable">变化率</span>与函数的走向。</p><svg class="figure" viewBox="0 0 460 160" role="img" aria-label="自制曲线与切线示意"><path d="M35 135h395M55 145V15" fill="none" stroke="#c9d2dc" stroke-width="1.2"/><path d="M70 130Q170 123 240 70T410 26" fill="none" stroke="#6b88a3" stroke-width="2"/><path d="m139 140 202-132" fill="none" stroke="#acbdcc" stroke-width="1.5" stroke-dasharray="5 4"/><circle cx="240" cy="70" r="4" fill="#6b88a3"/><text x="255" y="64" font-size="11" fill="#8192a2">局部斜率</text><text x="420" y="151" font-size="11" fill="#8192a2">x</text></svg><div class="figure-caption">图 ${number} · 在同一幅图像中观察曲线与局部线性近似</div><h2>${number}.3 留一个思考</h2><p>当函数值增长得越来越快时，切线如何变化？试着先描述图像，再回到公式。让符号与直觉互相检验。</p><div class="paper-number">${number}</div></article>`;
const thumbnailMarkup = (number) => `<button class="thumbnail ${number === 1 ? 'active' : ''}" data-action="page" data-number="${number}" aria-label="跳转到第 ${number} 页" ${number === 1 ? 'aria-current="page"' : ''}><span class="thumb-paper" aria-hidden="true"><b></b><i></i><i></i><i></i><i></i><em></em><i></i><i></i><i></i></span><span>${number}</span></button>`;

document.querySelector('#prototype').innerHTML = `
  <div class="design-bar"><strong>${layout === 'a' ? 'A · 经典横栏' : 'B · 侧轨阅读'}</strong><nav aria-label="设计方案"><a href="./index.html">比较</a><a href="./a.html" ${layout === 'a' ? 'class="current" aria-current="page"' : ''}>A</a><a href="./b.html" ${layout === 'b' ? 'class="current" aria-current="page"' : ''}>B</a></nav><span class="design-only">布局预览 · 非真实 PDF / AI</span><button class="quiet-button" data-action="about">说明</button></div>
  <main class="reader-shell" aria-label="阅读器布局预览">
    <header class="document-bar"><div class="app-mark" title="PDF Reader">${icon('file')}</div><div class="document-tabs" role="tablist" aria-label="文档标签"><div class="tab active" data-document="0"><button class="tab-select" role="tab" aria-selected="true" data-action="tab" data-document="0">${icon('file')}<span>微积分 · 示例.pdf</span></button>${button('close', '关闭微积分示例标签', 'close-tab', 'data-document="0"')}</div><div class="tab" data-document="1"><button class="tab-select" role="tab" aria-selected="false" data-action="tab" data-document="1">${icon('file')}<span>线性代数 · 示例.pdf</span></button>${button('close', '关闭线性代数示例标签', 'close-tab', 'data-document="1"')}</div></div><div class="document-actions">${button('folder', '打开文档（布局示意）', 'open')}${layout === 'b' ? '<button class="quiet-button ai-toggle" data-action="ai" aria-label="AI 侧栏（布局示意）" aria-expanded="false">' + icon('ai') + '<span>AI</span></button>' : ''}${layout === 'b' ? button('more', '更多操作', 'more', 'aria-expanded="false"') : ''}</div></header>
    ${layout === 'a' ? `<div class="reader-toolbar"><div class="toolbar-left">${button('thumbs', '缩略图侧栏', 'thumbs', 'aria-expanded="false"')}${button('search', '文档内搜索', 'search', 'aria-expanded="false"')}<span class="separator"></span><button class="quiet-button tool-mode" data-action="tools" aria-label="批注与框选入口（布局示意）" aria-expanded="false">${icon('pen')}<span class="mode-label">工具</span>${icon('down')}</button></div>${navigation()}<div class="toolbar-right"><button class="quiet-button ai-toggle" data-action="ai" aria-label="AI 侧栏（布局示意）" aria-expanded="false">${icon('ai')}<span>AI</span></button>${button('more', '更多操作', 'more', 'aria-expanded="false"')}</div></div>` : ''}
    <form class="search-strip" hidden aria-label="搜索布局示意"><input aria-label="搜索示意文档" placeholder="搜索文档" value="变化率" type="search"><span class="search-result" aria-live="polite">输入后按回车</span>${button('previous', '上一个搜索结果', 'search-previous')}${button('next', '下一个搜索结果', 'search-next')}${button('close', '关闭搜索', 'search-close')}</form>
    <div class="mode-strip" hidden><strong>框选区域 · 示意</strong><span>选定后再选择提问或解释</span><button class="quiet-button" data-action="region-end">退出</button></div>
    <div class="reader-body">
      ${layout === 'b' ? `<nav class="tool-rail" aria-label="阅读工具">${button('thumbs', '缩略图侧栏', 'thumbs', 'aria-expanded="false"')}${button('search', '文档内搜索', 'search', 'aria-expanded="false"')}<span class="separator"></span>${button('pen', '批注与框选入口（布局示意）', 'tools', 'aria-expanded="false"')}${button('region', '区域提问与原位解释（布局示意）', 'region')}<span class="rail-bottom"></span>${button('history', '阅读帮助与位置恢复说明', 'help')}</nav>` : ''}
      <aside class="thumbnail-panel" hidden aria-label="缩略图"><header class="panel-title"><span>缩略图</span>${button('close', '收起缩略图', 'thumbs-close')}</header><div class="thumbnail-list">${Array.from({ length: 8 }, (_, i) => thumbnailMarkup(i + 1)).join('')}</div></aside>
      <section class="reading-stage" aria-label="文档阅读区"><div class="page-scroller" tabindex="0" aria-label="可滚动的示意文档">${Array.from({ length: 8 }, (_, i) => pageMarkup(i + 1)).join('')}</div>${layout === 'b' ? `<footer class="navigation-dock">${navigation()}</footer>` : ''}</section>
      <aside class="ai-panel" hidden aria-label="AI 对话布局示意"><header class="panel-title"><span>AI 对话</span><div>${button('history', '文档对话历史（布局示意）', 'history')}${button('close', '收起 AI 侧栏', 'ai-close')}</div></header><div class="ai-config"><select aria-label="模型入口示意" disabled><option>主模型 · 待配置</option></select><select aria-label="风格入口示意" disabled><option>无风格 / Default</option></select></div><div class="context-row"><label><input type="checkbox" checked>当前 PDF</label><label title="正式功能需要单独授权"><input type="checkbox" disabled>同目录</label>${button('more', '上下文选择说明', 'context')}</div><div class="chat-content"><div class="context-chip" hidden>已选区域 · 第 1 页　<span>×</span></div><div class="chat-empty">${icon('ai')}<h3>带着问题阅读</h3><p>可以从当前文档开始，<br>也可以选定一段内容再提问。</p><p>这里仅展示未来对话布局。</p></div></div><div class="composer"><textarea placeholder="就当前文档提问…" aria-label="对话输入布局示意"></textarea><div class="composer-footer"><span>示意 · 不发送内容</span><button class="icon-button" aria-label="发送（布局示意，尚未接入）" disabled>${icon('send')}</button></div></div></aside>
    </div>
  </main>
  <div class="floating-menu" hidden></div><div class="toast" hidden role="status"></div>
  <dialog class="modal" aria-labelledby="modal-title"><h2 id="modal-title"></h2><p class="modal-text"></p><form method="dialog"><button>知道了</button></form></dialog>`;

const shell = document.querySelector('.reader-shell');
const scroller = document.querySelector('.page-scroller');
const menu = document.querySelector('.floating-menu');
const modal = document.querySelector('.modal');
let menuSource;
let activeDocument = 0;
let zoom = 100;
let zoomMode = '';
let currentPage = 1;
let searchIndex = -1;
let searchMatches = [];
const tabPositions = [{ page: 1, scroll: 0, zoom: 100, mode: '' }, { page: 1, scroll: 0, zoom: 100, mode: '' }];

function notify(message) {
  const toast = document.querySelector('.toast');
  toast.textContent = message;
  toast.hidden = false;
  clearTimeout(notify.timer);
  notify.timer = setTimeout(() => { toast.hidden = true; }, 3500);
}
function explain(title, message) {
  hideMenu();
  document.querySelector('#modal-title').textContent = title;
  document.querySelector('.modal-text').textContent = message;
  modal.showModal();
}
function hideMenu() {
  menu.hidden = true;
  menuSource?.setAttribute('aria-expanded', 'false');
}
function showMenu(source, html) {
  const wasOpen = !menu.hidden && menuSource === source;
  const anchor = menu.contains(source) ? menuSource : source;
  const rect = anchor.getBoundingClientRect();
  hideMenu();
  if (wasOpen) return;
  menuSource = anchor;
  menu.innerHTML = html;
  menu.hidden = false;
  anchor.setAttribute('aria-expanded', 'true');
  const left = Math.min(Math.max(8, rect.left), window.innerWidth - menu.offsetWidth - 8);
  const top = Math.min(rect.bottom + 5, window.innerHeight - menu.offsetHeight - 8);
  menu.style.left = `${left}px`;
  menu.style.top = `${Math.max(48, top)}px`;
  menu.querySelector('button, input')?.focus();
}
function togglePanel(selector, action, force) {
  const panel = document.querySelector(selector);
  panel.hidden = force === undefined ? !panel.hidden : !force;
  document.querySelector(`[data-action="${action}"]`).setAttribute('aria-expanded', String(!panel.hidden));
  if (!panel.hidden && window.innerWidth < 720) {
    const other = selector === '.ai-panel' ? '.thumbnail-panel' : '.ai-panel';
    document.querySelector(other).hidden = true;
    document.querySelector(`[data-action="${other === '.ai-panel' ? 'ai' : 'thumbs'}"]`).setAttribute('aria-expanded', 'false');
  }
}
function syncPage(number) {
  currentPage = number;
  document.querySelector('.page-input').value = number;
  document.querySelectorAll('.thumbnail').forEach((element) => {
    const active = Number(element.dataset.number) === number;
    element.classList.toggle('active', active);
    if (active) element.setAttribute('aria-current', 'page'); else element.removeAttribute('aria-current');
  });
  document.querySelector('[data-action="previous"]').disabled = number === 1;
  document.querySelector('[data-action="next"]').disabled = number === 8;
}
function goPage(number) {
  const next = Number(number);
  if (!Number.isInteger(next) || next < 1 || next > 8) {
    document.querySelector('.page-input').value = currentPage;
    notify('请输入 1–8 的整数页码。');
    return;
  }
  const page = document.querySelector(`[data-page="${next}"]`);
  scroller.scrollTop = page.offsetTop - 25;
  syncPage(next);
}
function setZoom(percent, mode = '') {
  zoom = Math.max(50, Math.min(200, Number(percent) || 100));
  zoomMode = mode;
  document.querySelector('.zoom-label').textContent = mode || `${zoom}%`;
  document.querySelectorAll('.paper').forEach((page) => {
    page.style.maxWidth = mode === '适合宽度' ? 'none' : `${650 * zoom / 100}px`;
    page.style.width = mode === '整页' ? `${Math.max(225, (scroller.clientHeight - 55) * .76)}px` : '100%';
    page.style.minHeight = mode === '整页' ? `${Math.max(550, scroller.clientHeight - 55)}px` : `${825 * zoom / 100}px`;
  });
  hideMenu();
}
function performSearch(direction = 1, newQuery = false) {
  const query = document.querySelector('.search-strip input').value.trim();
  if (newQuery) {
    document.querySelectorAll('.mock-match').forEach((match) => match.classList.remove('mock-match'));
    searchMatches = [...document.querySelectorAll('.searchable')].filter((span) => query && span.textContent.includes(query));
    searchMatches.forEach((match) => match.classList.add('mock-match'));
    searchIndex = -1;
  }
  if (!searchMatches.length) {
    document.querySelector('.search-result').textContent = query ? '无匹配（示意）' : '输入后按回车';
    return;
  }
  searchIndex = (searchIndex + direction + searchMatches.length) % searchMatches.length;
  const match = searchMatches[searchIndex];
  goPage(Number(match.closest('.paper').dataset.page));
  document.querySelector('.search-result').textContent = `${searchIndex + 1} / ${searchMatches.length}`;
}
function showRegion() {
  hideMenu();
  endRegion();
  const region = document.querySelector(`[data-page="${currentPage}"] .equation`);
  region.classList.add('selection-demo');
  if (!region.querySelector('.selection-actions')) region.insertAdjacentHTML('beforeend', `<div class="selection-actions" aria-label="区域操作示意"><button class="quiet-button" data-action="region-ask">${icon('ai')}提问</button><span class="separator"></span><button class="quiet-button" data-action="inline">快速解释</button></div>`);
  document.querySelector('.mode-strip').hidden = false;
}
function endRegion() {
  document.querySelector('.selection-actions')?.remove();
  document.querySelector('.selection-demo')?.classList.remove('selection-demo');
  document.querySelector('.mode-strip').hidden = true;
  document.querySelector('.inline-explanation')?.remove();
  document.querySelector('.context-chip').hidden = true;
}
function showInline() {
  if (document.querySelector('.inline-explanation')) return;
  document.querySelector('.selection-demo').closest('.paper').insertAdjacentHTML('beforeend', `<aside class="inline-explanation" aria-label="临时原位解释示意"><h3>局部变化的直觉 <small>临时 · 示意</small></h3><p>h 接近零时，两个点之间的割线逐渐接近切线。极限描述的是当前点的局部变化。</p><footer><button class="quiet-button" data-action="regenerate">再生成</button><button class="quiet-button" data-action="inline-cancel">取消</button><button class="quiet-button confirm" data-action="inline-confirm">确认</button></footer></aside>`);
}
function activateTab(number) {
  if (activeDocument === number) return;
  tabPositions[activeDocument] = { page: currentPage, scroll: scroller.scrollTop, zoom, mode: zoomMode };
  activeDocument = number;
  endRegion();
  document.querySelectorAll('.tab').forEach((tab) => {
    const active = Number(tab.dataset.document) === number;
    tab.classList.toggle('active', active);
    tab.querySelector('[role="tab"]').setAttribute('aria-selected', String(active));
  });
  document.querySelector('.paper h1').textContent = number === 0 ? titles[0] : '向量与线性组合';
  const state = tabPositions[number];
  setZoom(state.zoom, state.mode);
  scroller.scrollTop = state.scroll;
  syncPage(state.page);
}

document.addEventListener('click', (event) => {
  const target = event.target.closest('[data-action]');
  if (!target) { if (!event.target.closest('.floating-menu')) hideMenu(); return; }
  const action = target.dataset.action;
  const item = (name, label, value, hint = '') => `<button class="menu-item" data-action="${value}">${icon(name)}<span>${label}</span>${hint ? `<small>${hint}</small>` : ''}</button>`;
  if (action === 'thumbs' || action === 'thumbs-close') togglePanel('.thumbnail-panel', 'thumbs', action === 'thumbs-close' ? false : undefined);
  else if (action === 'ai' || action === 'ai-close') togglePanel('.ai-panel', 'ai', action === 'ai-close' ? false : undefined);
  else if (action === 'search' || action === 'search-close') {
    const search = document.querySelector('.search-strip');
    search.hidden = action === 'search-close' || !search.hidden;
    document.querySelector('[data-action="search"]').setAttribute('aria-expanded', String(!search.hidden));
    if (!search.hidden) search.querySelector('input').focus();
    else document.querySelectorAll('.mock-match').forEach((span) => span.classList.remove('mock-match'));
  }
  else if (action === 'previous') goPage(currentPage - 1);
  else if (action === 'next') goPage(currentPage + 1);
  else if (action === 'page') goPage(target.dataset.number);
  else if (action === 'zoom-in') setZoom(zoom + 10);
  else if (action === 'zoom-out') setZoom(zoom - 10);
  else if (action === 'zoom-menu') showMenu(target, `<h3>缩放与适合方式</h3>${item('width', '适合宽度', 'fit-width')}${item('fit', '整页', 'fit-page')}${item('file', '100%', 'actual-size')}<div class="menu-rule"></div><form class="custom-zoom"><input type="number" aria-label="自定义缩放百分比" min="50" max="200" value="${zoom}"><span>%</span><button class="quiet-button">应用</button></form>`);
  else if (action === 'fit-width') setZoom(100, '适合宽度');
  else if (action === 'fit-page') setZoom(100, '整页');
  else if (action === 'actual-size') setZoom(100);
  else if (action === 'more') showMenu(target, `${item('folder', '打开文档', 'open')}${layout === 'a' ? item('region', '批注与框选入口', 'tools', '示意') : ''}<div class="menu-rule"></div>${item('history', '阅读操作与位置恢复', 'help')}${item('file', '设计说明', 'about')}`);
  else if (action === 'tools') showMenu(target, `<h3>未来工具 · 仅交互示意</h3>${item('pen', '文字 / 高亮 / 笔画', 'annotations', '示意')}${item('region', '框选区域', 'region', '示意')}<div class="menu-rule"></div><h3>只在选定内容后展示操作</h3>`);
  else if (action === 'annotations') explain('批注模式', '未来进入批注模式后才展开文字、高亮与笔画工具；退出后回到阅读。未保存变化在标签中提示，Ctrl+S 或保存菜单显式写回。本预览不创建或保存批注。');
  else if (action === 'region') showRegion();
  else if (action === 'region-end') endRegion();
  else if (action === 'region-ask') {
    togglePanel('.ai-panel', 'ai', true);
    const chip = document.querySelector('.context-chip');
    chip.innerHTML = `<span>已选区域 · 第 ${document.querySelector('.selection-demo').closest('.paper').dataset.page} 页</span>${button('close', '移除区域上下文（布局示意）', 'context-remove')}`;
    chip.hidden = false;
    document.querySelector('.composer textarea').focus();
  }
  else if (action === 'context-remove') document.querySelector('.context-chip').hidden = true;
  else if (action === 'inline') showInline();
  else if (action === 'inline-cancel') document.querySelector('.inline-explanation')?.remove();
  else if (action === 'inline-confirm') explain('确认与保存分开', '正式功能中，确认会把临时解释变为待保存批注；随后仍需用户显式保存 PDF。本预览不会写入任何内容。');
  else if (action === 'regenerate') notify('原位解释的再生成入口示意，未调用模型。');
  else if (action === 'search-next' || action === 'search-previous') performSearch(action === 'search-next' ? 1 : -1, searchIndex < 0);
  else if (action === 'tab') activateTab(Number(target.dataset.document));
  else if (action === 'close-tab') {
    const tab = target.closest('.tab');
    const remaining = [...document.querySelectorAll('.tab')].find((element) => element !== tab);
    if (!remaining) { notify('最后一个标签的关闭后空态将在正式重构中沿用现有行为。'); return; }
    if (Number(tab.dataset.document) === activeDocument) activateTab(Number(remaining.dataset.document));
    tab.remove();
  }
  else if (action === 'open') explain('打开本地 PDF', '正式阅读器保留文件选择与拖入；这里使用自制页面展示布局，不读取文件。请从比较页的“返回现有阅读器”体验已完成的阅读功能。');
  else if (action === 'help') explain('阅读操作与位置恢复', '长篇说明移入帮助菜单。正式界面保留页码回车跳转、既有应用内快捷键和各标签独立位置；浏览器重启后需要重新选择文件，恢复提示只在需要时显示。');
  else if (action === 'history') explain('文档对话历史', '未来从这里查看当前文档的已提交对话与分支；不按文件名合并历史。这里只预留入口，不建立数据库或保存聊天。');
  else if (action === 'context') explain('上下文范围', '当前 PDF 默认开启；同目录默认关闭并需明确授权。缩略图点击用于阅读跳转；未来多页上下文选择通过专门模式展示复选状态，避免与当前页高亮混淆。');
  else if (action === 'about') explain('布局预览', '两个方案共用浅色、克制的视觉方向。可以展开侧栏、搜索、缩放与工具菜单。页面、AI 对话、区域与原位解释均是设计示意；没有读取 PDF、保存文件或调用 API。颜色、字体和动画尚未锁定。');
});

document.querySelector('.page-input').addEventListener('keydown', (event) => { if (event.key === 'Enter') goPage(event.target.value); });
document.querySelector('.search-strip').addEventListener('submit', (event) => { event.preventDefault(); performSearch(1, true); });
document.addEventListener('submit', (event) => { if (event.target.classList.contains('custom-zoom')) { event.preventDefault(); setZoom(event.target.querySelector('input').value); } });
scroller.addEventListener('scroll', () => {
  const pages = [...document.querySelectorAll('.paper')];
  const visible = pages.findLast((page) => page.offsetTop <= scroller.scrollTop + 110) || pages[0];
  syncPage(Number(visible.dataset.page));
}, { passive: true });
document.addEventListener('keydown', (event) => {
  if (event.key !== 'Escape') return;
  if (!menu.hidden) { hideMenu(); menuSource?.focus(); }
  else if (!document.querySelector('.mode-strip').hidden) endRegion();
  else if (!document.querySelector('.search-strip').hidden) { document.querySelector('[data-action="search-close"]').click(); document.querySelector('[data-action="search"]').focus(); }
});
window.addEventListener('resize', () => {
  hideMenu();
  if (window.innerWidth < 720 && !document.querySelector('.thumbnail-panel').hidden && !document.querySelector('.ai-panel').hidden) togglePanel('.thumbnail-panel', 'thumbs', false);
  if (zoomMode) setZoom(zoom, zoomMode);
});
syncPage(1);
// No reader state is touched. All interactions above are transient mock state.
shell.dataset.preview = 'layout-only';
