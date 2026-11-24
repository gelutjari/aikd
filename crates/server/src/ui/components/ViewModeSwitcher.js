/* ============================================================
   AIKD Knowledge Graph — view mode switcher
   Dark neon segmented control with 2D/structured + 3D groups.
   Registers: window.AIKD_GRAPH.ViewModeSwitcher
   ============================================================ */
(function (G) {
  'use strict';

  var STYLE_ID = 'aikd-view-switcher-styles';

  var GROUPS = [
    {
      id: 'structured',
      label: '2D & Structured',
      modes: [
        { key: 'galaxy', label: '2D Galaxy' },
        { key: 'tree', label: 'Hierarchy Tree' },
        { key: 'treemap', label: 'Treemap / 3D City' },
        { key: 'packing', label: 'Circle Packing' },
        { key: 'sunburst', label: 'Sunburst' },
        { key: 'islands', label: 'Semantic Islands' },
        { key: 'miller', label: 'Miller Columns' },
        { key: 'timeline', label: 'Swimlane Timeline' }
      ]
    },
    {
      id: 'spatial',
      label: '3D Spatial',
      modes: [
        { key: '3d-galaxy', label: '3D Force Galaxy' },
        { key: '3d-sphere', label: '3D Fibonacci Sphere' },
        { key: '3d-orbital', label: '3D Orbital Rings' },
        { key: '3d-layers', label: '3D Semantic Layers' }
      ]
    }
  ];

  var MODES = [];
  GROUPS.forEach(function (group) {
    group.modes.forEach(function (mode) {
      MODES.push({ key: mode.key, label: mode.label, group: group.id });
    });
  });

  var root = null;
  var activeMode = null;
  var changeHandler = null;
  var onClick = null;

  function ensureStyle() {
    if (document.getElementById(STYLE_ID)) return;
    var style = document.createElement('style');
    style.id = STYLE_ID;
    style.textContent = [
      '.aikd-vms{display:flex;flex-wrap:wrap;align-items:center;gap:8px 16px;padding:7px 10px;',
      'border:1px solid rgba(56,189,248,.18);border-radius:12px;',
      'background:linear-gradient(180deg,rgba(11,19,32,.94),rgba(7,12,20,.88));',
      'box-shadow:inset 0 1px 0 rgba(255,255,255,.04),0 10px 26px rgba(0,0,0,.34);}',
      '.aikd-vms-group{display:flex;align-items:center;gap:7px;flex-wrap:wrap;min-width:0;}',
      '.aikd-vms-label{font-size:9px;font-weight:700;letter-spacing:.14em;text-transform:uppercase;',
      'color:#64748b;padding:0 2px;white-space:nowrap;}',
      '.aikd-vms-pills{display:flex;flex-wrap:wrap;gap:4px;min-width:0;}',
      '.aikd-vms-pill{appearance:none;-webkit-appearance:none;border:1px solid rgba(148,163,184,.16);',
      'background:rgba(15,23,42,.55);color:#94a3b8;font:600 10.5px/1.1 inherit;padding:6px 9px;',
      'border-radius:999px;cursor:pointer;white-space:nowrap;transition:color .16s ease,',
      'border-color .16s ease,background .16s ease,box-shadow .16s ease,transform .16s ease;}',
      '.aikd-vms-pill:hover{color:#e2e8f0;border-color:rgba(56,189,248,.42);',
      'background:rgba(30,41,59,.72);transform:translateY(-1px);}',
      '.aikd-vms-pill:active{transform:translateY(0);}',
      '.aikd-vms-pill:focus-visible{outline:2px solid #38bdf8;outline-offset:1px;}',
      '.aikd-vms-pill.is-active{color:#04121d;border-color:transparent;',
      'background:linear-gradient(135deg,#22d3ee 0%,#818cf8 58%,#a78bfa 100%);',
      'box-shadow:0 0 14px rgba(34,211,238,.38),0 0 26px rgba(167,139,250,.22);}',
      '@media (max-width:720px){.aikd-vms{gap:6px 10px;padding:6px 8px;}',
      '.aikd-vms-pill{padding:5px 7px;font-size:10px;}}'
    ].join('');
    document.head.appendChild(style);
  }

  function setActive(mode) {
    activeMode = mode || null;
    if (!root) return;
    var buttons = root.querySelectorAll('.aikd-vms-pill');
    for (var i = 0; i < buttons.length; i += 1) {
      var button = buttons[i];
      var isActive = button.getAttribute('data-mode') === activeMode;
      button.classList.toggle('is-active', isActive);
      button.setAttribute('aria-pressed', isActive ? 'true' : 'false');
    }
  }

  function destroy() {
    if (!root) return;
    if (onClick) root.removeEventListener('click', onClick);
    if (root.parentNode) root.parentNode.removeChild(root);
    root = null;
    activeMode = null;
    changeHandler = null;
    onClick = null;
  }

  function mount(hostEl, initialMode, onChange) {
    if (!hostEl || typeof hostEl.appendChild !== 'function') return null;
    ensureStyle();
    destroy();
    changeHandler = typeof onChange === 'function' ? onChange : null;

    root = document.createElement('div');
    root.className = 'aikd-vms';
    root.setAttribute('role', 'tablist');
    root.setAttribute('aria-label', 'Knowledge graph view mode');

    GROUPS.forEach(function (group) {
      var groupEl = document.createElement('div');
      groupEl.className = 'aikd-vms-group';

      var labelEl = document.createElement('span');
      labelEl.className = 'aikd-vms-label';
      labelEl.textContent = group.label;
      groupEl.appendChild(labelEl);

      var pillsEl = document.createElement('div');
      pillsEl.className = 'aikd-vms-pills';
      group.modes.forEach(function (mode) {
        var button = document.createElement('button');
        button.type = 'button';
        button.className = 'aikd-vms-pill';
        button.setAttribute('data-mode', mode.key);
        button.setAttribute('role', 'tab');
        button.setAttribute('aria-pressed', 'false');
        button.title = mode.label;
        button.textContent = mode.label;
        pillsEl.appendChild(button);
      });
      groupEl.appendChild(pillsEl);
      root.appendChild(groupEl);
    });

    onClick = function (event) {
      var target = event.target;
      var button = target && target.closest ? target.closest('.aikd-vms-pill') : null;
      if (!button || !root || !root.contains(button)) return;
      var mode = button.getAttribute('data-mode');
      if (!mode || mode === activeMode) return;
      setActive(mode);
      if (changeHandler) changeHandler(mode);
    };
    root.addEventListener('click', onClick);

    hostEl.appendChild(root);
    setActive(initialMode || MODES[0].key);
    return root;
  }

  G.ViewModeSwitcher = {
    mount: mount,
    setActive: setActive,
    destroy: destroy,
    groups: GROUPS,
    modes: MODES,
    getActive: function () { return activeMode; },
    keys: MODES.map(function (mode) { return mode.key; })
  };
})(window.AIKD_GRAPH = window.AIKD_GRAPH || { views: {} });
