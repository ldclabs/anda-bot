import type {ReactNode} from 'react';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';

import styles from './index.module.css';

type HomeCopy = {
  meta: {title: string; description: string};
  hero: {badge: string; title: string; body: string; primary: string; secondary: string};
  proofs: Array<{label: string; detail: string}>;
  phases: Array<{index: string; title: string; body: string; to: string}>;
  docs: {badge: string; title: string; body: string};
  routes: {badge: string; title: string; items: Array<[string, string]>};
};

const copies: Record<string, HomeCopy> = {
  en: {
    meta: {
      title: 'Anda Bot Docs - Local memory-first AI assistant',
      description:
        'Official docs for installing the Anda Bot launcher, pairing the browser side panel, configuring the local daemon, and running long-term Brain graph memory.',
    },
    hero: {
      badge: 'Memory-first local AI assistant',
      title: 'Build around memory you own',
      body: 'Use these docs to install the app that owns the memory, connect the browser side panel, configure providers, and keep one local Brain across terminal, tools, channels, cron, and subagents.',
      primary: 'Install app',
      secondary: 'Pair browser',
    },
    proofs: [
      {label: 'memory-first', detail: 'local graph memory, not one model account'},
      {label: 'portable', detail: 'swap models without rebuilding context'},
      {label: 'daily surfaces', detail: 'browser, launcher, terminal, cron, skills, and IM share one Brain'},
    ],
    phases: [
      {
        index: '01',
        title: 'Install the app',
        body: 'Use the launcher, release installers, or CLI paths to set up the daemon, provider, model, and local home directory.',
        to: '/docs/quick-start/install',
      },
      {
        index: '02',
        title: 'Pair the browser',
        body: 'Generate a gateway URL and bearer token, then connect the side panel to the same local runtime and Brain.',
        to: '/docs/quick-start/browser-extension',
      },
      {
        index: '03',
        title: 'Understand Brain',
        body: 'Learn how Brain forms, recalls, and maintains a Cognitive Nexus for projects, preferences, relationships, and decisions.',
        to: '/docs/memory/brain',
      },
    ],
    docs: {
      badge: 'Documentation map',
      title: 'Start with the local app, then connect every surface to the same Brain.',
      body: 'Move from install and provider setup into browser pairing, terminal workflows, memory, subagents, channels, voice, and local data boundaries.',
    },
    routes: {
      badge: 'One local runtime',
      title: 'Browser, launcher, terminal, files, cron, skills, and IM channels can feed one memory thread.',
      items: [
        ['launcher', 'setup, status, pairing, logs, restart, and updates'],
        ['browser', 'page context, screenshots, downloads, and approved actions'],
        ['terminal', 'commands, files, skills, subagents, and local workspaces'],
        ['channels', 'Telegram, WeChat, Discord, Lark/Feishu, and voice'],
      ],
    },
  },
  'zh-Hans': {
    meta: {
      title: 'Anda Bot 文档 - 记忆优先的本地 AI 助手',
      description: 'Anda Bot 官方文档：安装桌面启动器，连接浏览器侧边栏，配置本地 daemon，并运行长期 Brain 知识图谱记忆。',
    },
    hero: {
      badge: '记忆优先的本地 AI 助手',
      title: '围绕你真正拥有的记忆构建',
      body: '这套文档会帮助你安装真正掌握记忆的本地应用，连接浏览器侧边栏，配置模型服务商，并让终端、工具、频道、定时任务和 Subagents 共享同一个 Brain。',
      primary: '获取应用',
      secondary: '连接浏览器',
    },
    proofs: [
      {label: '记忆优先', detail: '以本地图谱为核心，而不是绑定单一模型账号'},
      {label: '可迁移', detail: '自由切换模型，无需重建上下文和偏好'},
      {label: '日常入口', detail: '浏览器、启动器、终端、Cron、Skills 和消息频道共享 Brain'},
    ],
    phases: [
      {
        index: '01',
        title: '安装本地应用',
        body: '通过启动器、发布版安装器或命令行路径，完成 daemon、模型服务商、模型和本地目录配置。',
        to: '/docs/quick-start/install',
      },
      {
        index: '02',
        title: '连接浏览器',
        body: '生成 Gateway URL 和 Bearer token，把侧边栏连接到同一个本地运行时和 Brain。',
        to: '/docs/quick-start/browser-extension',
      },
      {
        index: '03',
        title: '理解 Brain',
        body: '了解 Brain 如何形成、召回和维护关于项目、偏好、关系与决策的 Cognitive Nexus。',
        to: '/docs/memory/brain',
      },
    ],
    docs: {
      badge: '文档地图',
      title: '先安装本地应用，再把每个入口连接到同一个 Brain。',
      body: '文档从安装和模型配置开始，继续覆盖浏览器配对、终端工作流、记忆机制、Subagents、频道、语音和本地数据边界。',
    },
    routes: {
      badge: '同一个本地运行时',
      title: '浏览器、启动器、终端、文件、Cron、Skills 和消息频道，都可以进入同一条记忆线索。',
      items: [
        ['launcher', '设置、状态、配对、日志、重启和更新'],
        ['browser', '页面上下文、截图、下载和授权操作'],
        ['terminal', '命令、文件、Skills、Subagents 和本地工作区'],
        ['channels', 'Telegram、WeChat、Discord、Lark/飞书和语音'],
      ],
    },
  },
  es: {
    meta: {title: 'Docs de Anda Bot - Asistente local con memoria primero', description: 'Documentación oficial para instalar el launcher, conectar el panel lateral del navegador, configurar el daemon local y usar memoria Brain.'},
    hero: {badge: 'Asistente local con memoria primero', title: 'Construye sobre memoria propia', body: 'Usa estos documentos para instalar la app que posee la memoria, conectar el navegador, configurar proveedores y mantener un Brain local entre terminal, herramientas, canales, cron y subagentes.', primary: 'Instalar app', secondary: 'Conectar navegador'},
    proofs: [{label: 'memory-first', detail: 'memoria local, no una cuenta de modelo'}, {label: 'portable', detail: 'cambia modelos sin reconstruir contexto'}, {label: 'superficies', detail: 'navegador, launcher, terminal, cron, skills e IM comparten Brain'}],
    phases: [{index: '01', title: 'Instalar la app', body: 'Configura launcher, daemon, proveedor, modelo y directorio local desde instaladores o CLI.', to: '/docs/quick-start/install'}, {index: '02', title: 'Conectar navegador', body: 'Genera Gateway URL y Bearer token para unir el panel lateral al mismo runtime local.', to: '/docs/quick-start/browser-extension'}, {index: '03', title: 'Entender Brain', body: 'Aprende cómo Brain forma, recuerda y mantiene proyectos, preferencias, relaciones y decisiones.', to: '/docs/memory/brain'}],
    docs: {badge: 'Mapa de documentación', title: 'Empieza con la app local y conecta cada superficie al mismo Brain.', body: 'Avanza desde instalación y modelos hacia navegador, terminal, memoria, subagentes, canales, voz y límites de datos locales.'},
    routes: {badge: 'Un runtime local', title: 'Navegador, launcher, terminal, archivos, cron, skills y canales IM pueden alimentar el mismo hilo de memoria.', items: [['launcher', 'setup, estado, pairing, logs, reinicio y updates'], ['browser', 'contexto de página, capturas, descargas y acciones aprobadas'], ['terminal', 'comandos, archivos, skills, subagentes y workspaces'], ['channels', 'Telegram, WeChat, Discord, Lark/Feishu y voz']]},
  },
  fr: {
    meta: {title: 'Docs Anda Bot - Assistant local centré mémoire', description: 'Documentation officielle pour installer le lanceur, connecter le panneau navigateur, configurer le daemon local et utiliser Brain.'},
    hero: {badge: 'Assistant local centré mémoire', title: 'Construire autour d’une mémoire qui vous appartient', body: 'Utilisez ces docs pour installer l’app qui possède la mémoire, connecter le navigateur, configurer les providers et garder un Brain local entre terminal, outils, canaux, cron et sous-agents.', primary: 'Installer l’app', secondary: 'Connecter le navigateur'},
    proofs: [{label: 'memory-first', detail: 'mémoire locale, pas un compte modèle'}, {label: 'portable', detail: 'changez de modèle sans reconstruire le contexte'}, {label: 'surfaces', detail: 'navigateur, lanceur, terminal, cron, skills et IM partagent Brain'}],
    phases: [{index: '01', title: 'Installer l’app', body: 'Configurez lanceur, daemon, provider, modèle et dossier local depuis les installateurs ou la CLI.', to: '/docs/quick-start/install'}, {index: '02', title: 'Connecter le navigateur', body: 'Générez Gateway URL et Bearer token pour relier le panneau au même runtime local.', to: '/docs/quick-start/browser-extension'}, {index: '03', title: 'Comprendre Brain', body: 'Découvrez comment Brain forme, rappelle et maintient projets, préférences, relations et décisions.', to: '/docs/memory/brain'}],
    docs: {badge: 'Carte documentaire', title: 'Commencez par l’app locale, puis reliez chaque surface au même Brain.', body: 'Passez de l’installation et des modèles au navigateur, terminal, mémoire, sous-agents, canaux, voix et limites locales.'},
    routes: {badge: 'Un runtime local', title: 'Navigateur, lanceur, terminal, fichiers, cron, skills et canaux IM peuvent nourrir le même fil de mémoire.', items: [['launcher', 'setup, statut, pairing, logs, redémarrage et mises à jour'], ['browser', 'contexte page, captures, téléchargements et actions approuvées'], ['terminal', 'commandes, fichiers, skills, sous-agents et workspaces'], ['channels', 'Telegram, WeChat, Discord, Lark/Feishu et voix']]},
  },
  ru: {
    meta: {title: 'Документация Anda Bot - локальный AI с памятью', description: 'Официальные docs для установки launcher, подключения browser side panel, настройки local daemon и работы с Brain memory.'},
    hero: {badge: 'Локальный помощник с memory-first подходом', title: 'Стройте вокруг памяти, которой владеете', body: 'Эти docs помогают установить app, владеющую памятью, подключить браузер, настроить providers и держать один local Brain для terminal, tools, channels, cron и subagents.', primary: 'Установить app', secondary: 'Подключить браузер'},
    proofs: [{label: 'memory-first', detail: 'локальная graph memory, не аккаунт модели'}, {label: 'portable', detail: 'меняйте модели без пересборки контекста'}, {label: 'surfaces', detail: 'browser, launcher, terminal, cron, skills и IM делят Brain'}],
    phases: [{index: '01', title: 'Установить app', body: 'Настройте launcher, daemon, provider, model и local home через installer или CLI.', to: '/docs/quick-start/install'}, {index: '02', title: 'Подключить браузер', body: 'Создайте Gateway URL и Bearer token, чтобы side panel вошла в тот же local runtime.', to: '/docs/quick-start/browser-extension'}, {index: '03', title: 'Понять Brain', body: 'Узнайте, как Brain формирует, вспоминает и поддерживает projects, preferences, relationships и decisions.', to: '/docs/memory/brain'}],
    docs: {badge: 'Карта документации', title: 'Начните с local app, затем подключите каждую поверхность к одному Brain.', body: 'Дальше идут установка, модели, браузер, terminal workflows, memory, subagents, channels, voice и local data boundaries.'},
    routes: {badge: 'Один local runtime', title: 'Browser, launcher, terminal, files, cron, skills и IM channels могут питать одну memory thread.', items: [['launcher', 'setup, status, pairing, logs, restart и updates'], ['browser', 'page context, screenshots, downloads и approved actions'], ['terminal', 'commands, files, skills, subagents и workspaces'], ['channels', 'Telegram, WeChat, Discord, Lark/Feishu и voice']]},
  },
  ar: {
    meta: {title: 'وثائق Anda Bot - مساعد محلي يبدأ من الذاكرة', description: 'وثائق رسمية لتثبيت launcher، وصل لوحة المتصفح، إعداد daemon المحلي، وتشغيل ذاكرة Brain الرسومية.'},
    hero: {badge: 'مساعد محلي يبدأ من الذاكرة', title: 'ابن حول ذاكرة تملكها أنت', body: 'استخدم هذه الوثائق لتثبيت التطبيق الذي يملك الذاكرة، وصل المتصفح، إعداد مزودي النماذج، وإبقاء Brain محلي واحد بين الطرفية والأدوات والقنوات و cron و subagents.', primary: 'ثبّت التطبيق', secondary: 'وصل المتصفح'},
    proofs: [{label: 'memory-first', detail: 'ذاكرة رسومية محلية، لا حساب نموذج واحد'}, {label: 'portable', detail: 'غيّر النماذج دون إعادة بناء السياق'}, {label: 'surfaces', detail: 'المتصفح و launcher والطرفية و cron و skills و IM تشارك Brain'}],
    phases: [{index: '01', title: 'ثبّت التطبيق', body: 'أعد launcher و daemon والمزوّد والنموذج والدليل المحلي عبر المثبتات أو CLI.', to: '/docs/quick-start/install'}, {index: '02', title: 'وصل المتصفح', body: 'أنشئ Gateway URL و Bearer token لوصل اللوحة الجانبية بنفس runtime المحلي.', to: '/docs/quick-start/browser-extension'}, {index: '03', title: 'افهم Brain', body: 'تعلّم كيف يشكل Brain ويسترجع ويحافظ على المشاريع والتفضيلات والعلاقات والقرارات.', to: '/docs/memory/brain'}],
    docs: {badge: 'خريطة الوثائق', title: 'ابدأ بالتطبيق المحلي، ثم صل كل سطح بنفس Brain.', body: 'انتقل من التثبيت والنماذج إلى المتصفح والطرفية والذاكرة و subagents والقنوات والصوت وحدود البيانات المحلية.'},
    routes: {badge: 'runtime محلي واحد', title: 'يمكن للمتصفح و launcher والطرفية والملفات و cron و skills وقنوات IM تغذية خيط ذاكرة واحد.', items: [['launcher', 'الإعداد والحالة والربط والسجلات وإعادة التشغيل والتحديثات'], ['browser', 'سياق الصفحة واللقطات والتنزيلات والأفعال المصرح بها'], ['terminal', 'أوامر وملفات و skills و subagents ومساحات عمل'], ['channels', 'Telegram وWeChat وDiscord وLark/Feishu والصوت']]},
  },
};

export default function Home(): ReactNode {
  const {i18n} = useDocusaurusContext();
  const copy = copies[i18n.currentLocale] ?? copies.en;

  return (
    <Layout
      title={copy.meta.title}
      description={copy.meta.description}>
      <main className={styles.page}>
        <section className={styles.hero}>
          <div className={styles.texture} aria-hidden="true" />
          <div className={styles.scan} aria-hidden="true" />
          <div className={styles.heroInner}>
            <div className={styles.heroCopy}>
              <p className={styles.badge}>{copy.hero.badge}</p>
              <h1 className={styles.title}>{copy.hero.title}</h1>
              <p className={styles.lede}>{copy.hero.body}</p>
              <div className={styles.actions}>
                <Link className={`${styles.cta} ${styles.ctaPrimary}`} to="/docs/quick-start/install">
                  {copy.hero.primary}
                </Link>
                <Link className={`${styles.cta} ${styles.ctaSecondary}`} to="/docs/quick-start/browser-extension">
                  {copy.hero.secondary}
                </Link>
              </div>
            </div>

            <div className={styles.proofGrid}>
              {copy.proofs.map((proof) => (
                <span key={proof.label}>
                  <strong>{proof.label}</strong>
                  {proof.detail}
                </span>
              ))}
            </div>
          </div>
        </section>

        <section className={styles.docsBand}>
          <div className={styles.sectionHeader}>
            <p className={styles.badgeDark}>{copy.docs.badge}</p>
            <h2>{copy.docs.title}</h2>
            <p>{copy.docs.body}</p>
          </div>

          <div className={styles.phaseGrid}>
            {copy.phases.map((phase) => (
              <Link className={styles.phaseCard} to={phase.to} key={phase.title}>
                <span>{phase.index}</span>
                <h3>{phase.title}</h3>
                <p>{phase.body}</p>
              </Link>
            ))}
          </div>
        </section>

        <section className={styles.routesBand}>
          <div className={styles.routesPanel}>
            <div>
              <p className={styles.badge}>{copy.routes.badge}</p>
              <h2>{copy.routes.title}</h2>
            </div>
            <div className={styles.routeGrid}>
              {copy.routes.items.map(([name, detail]) => (
                <div className={styles.routeItem} key={name}>
                  <span>{name}</span>
                  <p>{detail}</p>
                </div>
              ))}
            </div>
          </div>
        </section>
      </main>
    </Layout>
  );
}
