import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

const config: Config = {
  title: 'Anda Bot Docs',
  tagline: 'Open-source Rust agent with graph memory, tools, and long-horizon goals.',
  favicon: 'img/logo.svg',

  // Future flags, see https://docusaurus.io/docs/api/docusaurus-config#future
  future: {
    v4: true, // Improve compatibility with the upcoming Docusaurus v4
  },

  // Set the production url of your site here
  url: 'https://docs.anda.bot',
  // Set the /<baseUrl>/ pathname under which your site is served
  // For GitHub pages deployment, it is often '/<projectName>/'
  baseUrl: '/',

  // GitHub pages deployment config.
  // If you aren't using GitHub pages, you don't need these.
  organizationName: 'ldclabs',
  projectName: 'anda-bot',

  onBrokenLinks: 'throw',
  markdown: {
    hooks: {
      onBrokenMarkdownLinks: 'warn',
    },
  },

  i18n: {
    defaultLocale: 'en',
    locales: ['en', 'zh-Hans', 'es', 'fr', 'ru', 'ar'],
    localeConfigs: {
      en: {label: 'English', htmlLang: 'en'},
      'zh-Hans': {label: '中文', htmlLang: 'zh-CN'},
      es: {label: 'Español', htmlLang: 'es'},
      fr: {label: 'Français', htmlLang: 'fr'},
      ru: {label: 'Русский', htmlLang: 'ru'},
      ar: {label: 'العربية', htmlLang: 'ar', direction: 'rtl'},
    },
  },

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          editUrl:
            'https://github.com/ldclabs/anda-bot/tree/main/docsite/',
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'img/anda_bot.webp',
    metadata: [
      {
        name: 'description',
        content:
          'Anda Bot documentation: install the open-source Rust agent, configure models and graph memory, and connect terminals, tools, subagents, and team channels.',
      },
      {property: 'og:type', content: 'website'},
      {property: 'og:url', content: 'https://docs.anda.bot'},
      {name: 'twitter:card', content: 'summary_large_image'},
    ],
    colorMode: {
      defaultMode: 'dark',
      respectPrefersColorScheme: false,
    },
    navbar: {
      title: 'Anda Bot Docs',
      logo: {
        alt: 'Anda Bot',
        src: 'img/logo.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docsSidebar',
          position: 'left',
          label: 'Docs',
        },
        {to: '/docs/quick-start/install', label: 'Quick Start', position: 'left'},
        {
          type: 'localeDropdown',
          position: 'right',
        },
        {
          href: 'https://anda.bot',
          label: 'Main Site',
          position: 'right',
        },
        {
          href: 'https://github.com/ldclabs/anda-bot',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Start',
          items: [
            {
              label: 'Overview',
              to: '/docs/intro',
            },
            {
              label: 'Install Anda',
              to: '/docs/quick-start/install',
            },
            {
              label: 'Terminal',
              to: '/docs/quick-start/terminal',
            },
          ],
        },
        {
          title: 'Core',
          items: [
            {
              label: 'Hippocampus Memory',
              to: '/docs/memory/hippocampus',
            },
            {
              label: 'Long-Horizon Work',
              to: '/docs/workflows/long-horizon',
            },
            {
              label: 'Channels and Voice',
              to: '/docs/runtime/channels',
            },
          ],
        },
        {
          title: 'Project',
          items: [
            {
              label: 'Anda Bot',
              href: 'https://github.com/ldclabs/anda-bot',
            },
            {
              label: 'Anda Hippocampus',
              href: 'https://github.com/ldclabs/anda-hippocampus',
            },
            {
              label: 'Product Site',
              href: 'https://anda.bot',
            },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} LDC Labs. Built with Docusaurus.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
