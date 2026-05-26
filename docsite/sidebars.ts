import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

const sidebars: SidebarsConfig = {
  docsSidebar: [
    'intro',
    {
      type: 'category',
      label: 'Quick Start',
      collapsed: false,
      items: ['quick-start/install', 'quick-start/terminal', 'quick-start/browser-extension'],
    },
    {
      type: 'category',
      label: 'Runtime',
      collapsed: false,
      items: ['runtime/configuration', 'runtime/channels'],
    },
    {
      type: 'category',
      label: 'Memory',
      collapsed: false,
      items: ['memory/brain'],
    },
    {
      type: 'category',
      label: 'Workflows',
      collapsed: false,
      items: ['workflows/long-horizon'],
    },
  ],
};

export default sidebars;
