import type {ReactNode} from 'react';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import useBaseUrl from '@docusaurus/useBaseUrl';
import Layout from '@theme/Layout';

import styles from './index.module.css';

type HomeCopy = {
  meta: {title: string; description: string};
  hero: {badge: string; title: string; body: string; primary: string; secondary: string};
  proofs: Array<{label: string; detail: string}>;
  phases: Array<{index: string; title: string; body: string; to: string}>;
  signals: Array<[string, string]>;
  docs: {badge: string; title: string; body: string};
  routes: {badge: string; title: string; items: Array<[string, string]>};
};

const copies: Record<string, HomeCopy> = {
  en: {
    meta: {
      title: 'Anda Bot Docs',
      description:
        'Official Anda Bot documentation for installing the open-source Rust agent, configuring graph memory, and connecting tools, subagents, and team channels.',
    },
    hero: {
      badge: 'docs.anda.bot',
      title: 'Anda Bot Docs',
      body: 'I am the local Rust agent that keeps learning while we work. These docs show how to install me, configure models, connect channels, use Hippocampus graph memory, and hand long-horizon goals to subagents and local tools.',
      primary: 'Start installing',
      secondary: 'Meet the memory brain',
    },
    proofs: [
      {label: 'Rust', detail: 'open-source terminal runtime'},
      {label: 'Graph', detail: 'long-term memory'},
      {label: 'Goal', detail: 'long-horizon loops'},
    ],
    phases: [
      {
        index: '01',
        title: 'Install and configure',
        body: 'Start Anda from a release or source, add a model provider, and connect the local daemon to the terminal UI.',
        to: '/docs/quick-start/install',
      },
      {
        index: '02',
        title: 'Long-term memory',
        body: 'Learn how Hippocampus forms, recalls, and maintains a knowledge graph instead of stretching one chat log forever.',
        to: '/docs/memory/hippocampus',
      },
      {
        index: '03',
        title: 'Long-horizon work',
        body: 'Use /goal, subagents, skills, shell, and external coding tools to turn one request into verified progress.',
        to: '/docs/workflows/long-horizon',
      },
    ],
    signals: [
      ['goal://active', 'long-horizon goal stays active'],
      ['anda://memory', 'Hippocampus forms and recalls'],
      ['tools://local', 'shell, files, skills, subagents'],
    ],
    docs: {
      badge: 'Documentation map',
      title: 'Turn one launch into a local agent that can keep working.',
      body: 'Start with installation, then move through the terminal UI, model configuration, memory, subagents, channels, and local data boundaries.',
    },
    routes: {
      badge: 'Where context is born',
      title: 'Terminal, tools, files, channels, and voice can enter the same memory thread.',
      items: [
        ['terminal', 'commands, files, logs, and local workspaces'],
        ['memory', 'projects, preferences, decisions, and timelines'],
        ['channels', 'IRC, Telegram, WeChat, Discord, and Feishu'],
        ['voice', 'transcription, TTS, and spoken input'],
      ],
    },
  },
  'zh-Hans': {
    meta: {
      title: 'Anda Bot 文档',
      description: 'Anda Bot 官方文档：安装开源 Rust Agent，配置模型和长期记忆，连接终端、工具、Subagents 与团队频道。',
    },
    hero: {
      badge: 'docs.anda.bot',
      title: 'Anda Bot 文档',
      body: '我是会记忆的本地 Rust Agent。这里记录如何安装我、配置模型、连接频道、使用 Hippocampus 长期记忆，并把长程目标交给 Subagents 和本地工具持续推进。',
      primary: '开始安装',
      secondary: '了解记忆大脑',
    },
    proofs: [
      {label: 'Rust', detail: '开源终端运行时'},
      {label: 'Graph', detail: '长期记忆'},
      {label: 'Goal', detail: '长程目标循环'},
    ],
    phases: [
      {
        index: '01',
        title: '安装和配置',
        body: '从发布版或源码启动 Anda，填好模型 provider，让本地 daemon 和终端 UI 连起来。',
        to: '/docs/quick-start/install',
      },
      {
        index: '02',
        title: '长期记忆',
        body: '理解 Hippocampus 如何形成、召回、维护知识图谱，而不是堆一份越来越长的聊天记录。',
        to: '/docs/memory/hippocampus',
      },
      {
        index: '03',
        title: '长程工作流',
        body: '用 /goal、Subagents、Skills、shell 和外部编码工具，把一次回答变成可验证的连续推进。',
        to: '/docs/workflows/long-horizon',
      },
    ],
    signals: [
      ['goal://active', '长程目标保持活跃'],
      ['anda://memory', 'Hippocampus 形成和召回'],
      ['tools://local', 'shell、文件、Skills、Subagents'],
    ],
    docs: {
      badge: 'Documentation map',
      title: '把一次启动，变成能长期工作的本地智能体。',
      body: '文档从安装路径开始，逐步展开到终端 UI、模型配置、记忆机制、Subagents、频道和本地数据边界。',
    },
    routes: {
      badge: 'Where context is born',
      title: '终端、工具、文件、频道和语音，都可以进入同一条记忆线索。',
      items: [
        ['terminal', '命令、文件、日志和本地工作区'],
        ['memory', '项目、偏好、决策和时间线'],
        ['channels', 'IRC、Telegram、WeChat、Discord、飞书'],
        ['voice', '转写、TTS 和语音输入'],
      ],
    },
  },
  es: {
    meta: {title: 'Documentación de Anda Bot', description: 'Documentación oficial de Anda Bot para instalar, configurar memoria gráfica y conectar herramientas, subagentes y canales.'},
    hero: {badge: 'docs.anda.bot', title: 'Documentación de Anda Bot', body: 'Soy el agente local en Rust que aprende mientras trabajamos. Esta documentación cubre instalación, modelos, canales, memoria Hippocampus y objetivos largos con subagentes y herramientas locales.', primary: 'Instalar', secondary: 'Ver la memoria'},
    proofs: [{label: 'Rust', detail: 'runtime terminal abierto'}, {label: 'Graph', detail: 'memoria a largo plazo'}, {label: 'Goal', detail: 'bucles de objetivos'}],
    phases: [{index: '01', title: 'Instalar y configurar', body: 'Inicia Anda desde una versión publicada o desde el código fuente y conecta el daemon local con la UI terminal.', to: '/docs/quick-start/install'}, {index: '02', title: 'Memoria duradera', body: 'Aprende cómo Hippocampus forma, recupera y mantiene un grafo de conocimiento.', to: '/docs/memory/hippocampus'}, {index: '03', title: 'Trabajo de largo alcance', body: 'Usa /goal, subagentes, skills, shell y herramientas externas para lograr progreso verificable.', to: '/docs/workflows/long-horizon'}],
    signals: [['goal://active', 'el objetivo sigue activo'], ['anda://memory', 'Hippocampus forma y recupera'], ['tools://local', 'shell, archivos, skills, subagentes']],
    docs: {badge: 'Mapa de documentación', title: 'Convierte una ejecución local en un agente que sigue trabajando.', body: 'Empieza con la instalación y continúa con terminal, modelos, memoria, subagentes, canales y límites de datos locales.'},
    routes: {badge: 'Donde nace el contexto', title: 'Terminal, herramientas, archivos, canales y voz pueden entrar en el mismo hilo de memoria.', items: [['terminal', 'comandos, archivos, registros y workspaces'], ['memory', 'proyectos, preferencias, decisiones y líneas de tiempo'], ['channels', 'IRC, Telegram, WeChat, Discord y Feishu'], ['voice', 'transcripción, TTS y entrada hablada']]},
  },
  fr: {
    meta: {title: 'Documentation Anda Bot', description: 'Documentation officielle d\'Anda Bot pour installer, configurer la mémoire graphe et connecter outils, sous-agents et canaux.'},
    hero: {badge: 'docs.anda.bot', title: 'Documentation Anda Bot', body: 'Je suis l\'agent Rust local qui apprend pendant le travail. Ces documents couvrent l\'installation, les modèles, les canaux, la mémoire Hippocampus et les objectifs longs avec sous-agents et outils locaux.', primary: 'Installer', secondary: 'Voir la mémoire'},
    proofs: [{label: 'Rust', detail: 'runtime terminal open source'}, {label: 'Graph', detail: 'mémoire long terme'}, {label: 'Goal', detail: 'boucles d\'objectifs'}],
    phases: [{index: '01', title: 'Installer et configurer', body: 'Démarre Anda depuis une version publiée ou le code source, puis connecte le daemon local à l\'interface terminal.', to: '/docs/quick-start/install'}, {index: '02', title: 'Mémoire durable', body: 'Comprends comment Hippocampus forme, rappelle et maintient un graphe de connaissances.', to: '/docs/memory/hippocampus'}, {index: '03', title: 'Travail longue durée', body: 'Utilise /goal, les sous-agents, skills, shell et outils externes pour obtenir un progrès vérifié.', to: '/docs/workflows/long-horizon'}],
    signals: [['goal://active', 'l\'objectif reste actif'], ['anda://memory', 'Hippocampus forme et rappelle'], ['tools://local', 'shell, fichiers, skills, sous-agents']],
    docs: {badge: 'Carte documentaire', title: 'Transforme un lancement local en agent capable de continuer le travail.', body: 'Commence par l\'installation, puis explore terminal, modèles, mémoire, sous-agents, canaux et limites des données locales.'},
    routes: {badge: 'Où naît le contexte', title: 'Terminal, outils, fichiers, canaux et voix peuvent entrer dans le même fil de mémoire.', items: [['terminal', 'commandes, fichiers, journaux et espaces locaux'], ['memory', 'projets, préférences, décisions et chronologies'], ['channels', 'IRC, Telegram, WeChat, Discord et Feishu'], ['voice', 'transcription, TTS et entrée vocale']]},
  },
  ru: {
    meta: {title: 'Документация Anda Bot', description: 'Официальная документация Anda Bot: установка, графовая память, инструменты, сабагенты и каналы команды.'},
    hero: {badge: 'docs.anda.bot', title: 'Документация Anda Bot', body: 'Я локальный Rust-агент, который учится во время работы. Здесь описаны установка, модели, каналы, память Hippocampus и долгие цели с сабагентами и локальными инструментами.', primary: 'Установить', secondary: 'Открыть память'},
    proofs: [{label: 'Rust', detail: 'открытый терминальный runtime'}, {label: 'Graph', detail: 'долговременная память'}, {label: 'Goal', detail: 'циклы долгих целей'}],
    phases: [{index: '01', title: 'Установка и настройка', body: 'Запустите Anda из релиза или исходников и подключите локальный daemon к терминальному UI.', to: '/docs/quick-start/install'}, {index: '02', title: 'Долгая память', body: 'Узнайте, как Hippocampus формирует, извлекает и поддерживает граф знаний.', to: '/docs/memory/hippocampus'}, {index: '03', title: 'Длинные задачи', body: 'Используйте /goal, сабагентов, skills, shell и внешние инструменты для проверяемого прогресса.', to: '/docs/workflows/long-horizon'}],
    signals: [['goal://active', 'цель остается активной'], ['anda://memory', 'Hippocampus формирует и вспоминает'], ['tools://local', 'shell, файлы, skills, сабагенты']],
    docs: {badge: 'Карта документации', title: 'Превратите один запуск в локального агента, который продолжает работу.', body: 'Начните с установки, затем изучите терминал, модели, память, сабагентов, каналы и локальные границы данных.'},
    routes: {badge: 'Где рождается контекст', title: 'Терминал, инструменты, файлы, каналы и голос могут войти в одну линию памяти.', items: [['terminal', 'команды, файлы, логи и локальные рабочие области'], ['memory', 'проекты, предпочтения, решения и временные линии'], ['channels', 'IRC, Telegram, WeChat, Discord и Feishu'], ['voice', 'транскрипция, TTS и голосовой ввод']]},
  },
  ar: {
    meta: {title: 'وثائق Anda Bot', description: 'وثائق Anda Bot الرسمية للتثبيت، وذاكرة الرسم البياني، والأدوات، والوكلاء الفرعيين، وقنوات الفريق.'},
    hero: {badge: 'docs.anda.bot', title: 'وثائق Anda Bot', body: 'أنا وكيل Rust محلي يتعلم أثناء العمل. تشرح هذه الوثائق التثبيت، والنماذج، والقنوات، وذاكرة Hippocampus، والأهداف طويلة المدى مع الوكلاء الفرعيين والأدوات المحلية.', primary: 'ابدأ التثبيت', secondary: 'تعرّف على الذاكرة'},
    proofs: [{label: 'Rust', detail: 'تشغيل طرفية مفتوح المصدر'}, {label: 'Graph', detail: 'ذاكرة طويلة المدى'}, {label: 'Goal', detail: 'حلقات أهداف طويلة'}],
    phases: [{index: '01', title: 'التثبيت والإعداد', body: 'شغّل Anda من إصدار جاهز أو من المصدر، ثم صِل daemon المحلي بواجهة الطرفية.', to: '/docs/quick-start/install'}, {index: '02', title: 'ذاكرة طويلة المدى', body: 'تعرّف على كيفية تكوين Hippocampus واسترجاعه وصيانته لرسم معرفي.', to: '/docs/memory/hippocampus'}, {index: '03', title: 'عمل طويل المدى', body: 'استخدم /goal والوكلاء الفرعيين والمهارات و shell والأدوات الخارجية للوصول إلى تقدم قابل للتحقق.', to: '/docs/workflows/long-horizon'}],
    signals: [['goal://active', 'يبقى الهدف نشطًا'], ['anda://memory', 'Hippocampus يكوّن ويسترجع'], ['tools://local', 'shell وملفات ومهارات ووكلاء فرعيون']],
    docs: {badge: 'خريطة الوثائق', title: 'حوّل تشغيلًا واحدًا إلى وكيل محلي يواصل العمل.', body: 'ابدأ بالتثبيت، ثم انتقل إلى الطرفية، والنماذج، والذاكرة، والوكلاء الفرعيين، والقنوات، وحدود البيانات المحلية.'},
    routes: {badge: 'حيث يولد السياق', title: 'يمكن للطرفية والأدوات والملفات والقنوات والصوت أن تدخل خيط الذاكرة نفسه.', items: [['terminal', 'أوامر وملفات وسجلات ومساحات عمل محلية'], ['memory', 'مشاريع وتفضيلات وقرارات وجداول زمنية'], ['channels', 'IRC وTelegram وWeChat وDiscord وFeishu'], ['voice', 'تفريغ صوتي وTTS وإدخال منطوق']]},
  },
};

export default function Home(): ReactNode {
  const {i18n} = useDocusaurusContext();
  const heroImage = useBaseUrl('/img/anda_bot.webp');
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
                <Link className={`${styles.cta} ${styles.ctaSecondary}`} to="/docs/memory/hippocampus">
                  {copy.hero.secondary}
                </Link>
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

            <div className={styles.observatory}>
              <div className={styles.imageFrame}>
                <img src={heroImage} alt="Anda Bot terminal interface" />
              </div>
              <div className={styles.signalPanel}>
                <div className={styles.signalHeader}>
                  <span>hippocampus.live</span>
                  <span>goal://active</span>
                </div>
                {copy.signals.map(([label, detail]) => (
                  <div className={styles.signalRow} key={label}>
                    <span>{label}</span>
                    <small>{detail}</small>
                  </div>
                ))}
              </div>
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
