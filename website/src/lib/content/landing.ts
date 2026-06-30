export type Locale = 'ar' | 'zh' | 'en' | 'fr' | 'ru' | 'es';
export type TextDirection = 'ltr' | 'rtl';
export type OsKey = 'macos' | 'windows' | 'linux';

type MetricCopy = {
	value: string;
	label: string;
};

type InstallOptionCopy = {
	label: string;
	title: string;
	body: string;
	primaryLabel: string;
	href?: string;
	download?: string;
	command?: string;
	commandLabel?: string;
	note: string;
	steps: [string, string, string];
};

type FeatureCopy = {
	title: string;
	detail: string;
};

type SurfaceCopy = {
	label: string;
	detail: string;
};

type WhyRouteCopy = {
	name: string;
	role: string;
	fit: string;
	primary?: boolean;
};

export type LandingCopy = {
	meta: {
		title: string;
		description: string;
		ogTitle: string;
		ogDescription: string;
	};
	nav: {
		install: string;
		why: string;
		browser: string;
		launcher: string;
		memory: string;
		docs: string;
	};
	language: {
		label: string;
	};
	hero: {
		badge: string;
		title: string;
		body: string;
		primary: string;
		secondary: string;
	};
	proof: MetricCopy[];
	why: {
		badge: string;
		title: string;
		body: string;
		routes: WhyRouteCopy[];
	};
	install: {
		badge: string;
		title: string;
		body: string;
		detected: string;
		chooseOs: string;
		tabAria: string;
		copy: string;
		copied: string;
		copyFailed: string;
		copyAria: string;
		commandAria: string;
		options: Record<OsKey, InstallOptionCopy>;
	};
	browser: {
		badge: string;
		title: string;
		body: string;
		store: string;
		docs: string;
		features: FeatureCopy[];
	};
	launcher: {
		badge: string;
		title: string;
		body: string;
		features: FeatureCopy[];
	};
	memory: {
		badge: string;
		title: string;
		body: string;
		features: FeatureCopy[];
	};
	work: {
		badge: string;
		title: string;
		body: string;
		surfaces: SurfaceCopy[];
	};
	final: {
		title: string;
		body: string;
		install: string;
		docs: string;
		github: string;
	};
};

export const fallbackLocale: Locale = 'en';

export const localeOrder: Locale[] = ['en', 'zh', 'es', 'fr', 'ru', 'ar'];

export const localeMeta: Record<
	Locale,
	{ label: string; nativeName: string; htmlLang: string; dir: TextDirection }
> = {
	ar: { label: 'Arabic', nativeName: 'العربية', htmlLang: 'ar', dir: 'rtl' },
	zh: { label: 'Chinese', nativeName: '中文', htmlLang: 'zh-CN', dir: 'ltr' },
	en: { label: 'English', nativeName: 'English', htmlLang: 'en', dir: 'ltr' },
	fr: { label: 'French', nativeName: 'Français', htmlLang: 'fr', dir: 'ltr' },
	ru: { label: 'Russian', nativeName: 'Русский', htmlLang: 'ru', dir: 'ltr' },
	es: { label: 'Spanish', nativeName: 'Español', htmlLang: 'es', dir: 'ltr' }
};

const windowsInstallerFileName = 'AndaBotSetup-windows-x86_64.exe';
const windowsInstallerUrl = `https://github.com/ldclabs/anda-bot/releases/latest/download/${windowsInstallerFileName}`;
const extensionStoreUrl =
	'https://chromewebstore.google.com/detail/anda-bot/injpfajmddchcphfkdkiflfddmajglfd';
const browserDocsUrl = 'https://docs.anda.bot/docs/quick-start/browser-extension';

export const landingCopy: Record<Locale, LandingCopy> = {
	en: {
		meta: {
			title: 'Anda Bot - Local memory-first AI assistant',
			description:
				'Use Anda Bot as the local memory-first assistant that keeps your graph memory, context, preferences, tools, and long tasks under your control.',
			ogTitle: 'Anda Bot - Local memory-first AI assistant',
			ogDescription:
				'Install the desktop launcher, connect the browser extension, and keep long-term graph memory on your own machine.'
		},
		nav: {
			install: 'Install app',
			why: 'Why Anda',
			browser: 'Browser',
			launcher: 'Launcher',
			memory: 'Memory',
			docs: 'Docs'
		},
		language: { label: 'Language' },
		hero: {
			badge: 'Memory-first local AI assistant',
			title: 'Your model can change. Your memory should not',
			body: 'Anda Bot keeps long-term graph memory on your machine, so your assistant survives platforms, models, and sessions.',
			primary: 'Install app',
			secondary: 'Add extension'
		},
		proof: [
			{
				value: 'memory-first',
				label: 'Built around local graph memory, not a single model account'
			},
			{ value: 'portable', label: 'Swap models without rebuilding your context and preferences' },
			{
				value: 'daily surfaces',
				label: 'Browser, launcher, terminal, skills, cron, and IM channels share one Brain'
			}
		],
		why: {
			badge: 'Why Anda Bot',
			title: 'Use code agents for code. Use Anda Bot for continuity',
			body: 'Claude Code and Codex are excellent inside a repo. Anda Bot is the long-lived assistant layer that remembers who you are across work.',
			routes: [
				{
					name: 'Claude Code and Codex',
					role: 'Focused coding sessions',
					fit: 'Best when the repository is the context and memory is optional after the task ends.'
				},
				{
					name: 'OpenClaw and Hermes-style platforms',
					role: 'Broad tool and plugin coverage',
					fit: 'Best when the priority is ecosystem breadth, packaged skills, and many ready-made capabilities.'
				},
				{
					name: 'Anda Bot',
					role: 'Personal assistant substrate',
					fit: 'Best when your preferences, relationships, research trails, routines, and identity need to survive model changes.',
					primary: true
				}
			]
		},
		install: {
			badge: 'Get started',
			title: 'Install the app that owns the memory',
			body: 'Start with the launcher, connect the browser, and keep the daemon plus Brain running locally.',
			detected: 'Detected {os}',
			chooseOs: 'Choose OS',
			tabAria: 'Install path by operating system',
			copy: 'Copy',
			copied: 'Copied',
			copyFailed: 'Copy failed',
			copyAria: 'Copy install command',
			commandAria: 'Copy the install command',
			options: {
				macos: {
					label: 'macOS',
					title: 'Menu-bar launcher',
					body: 'The install script adds Anda Bot.app, registers the menu-bar launcher at login, and starts the daemon after setup.',
					primaryLabel: 'Copy installer',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'macOS install script',
					note: 'The launcher can check for updates, restart the daemon, open logs, and create browser pairing tokens.',
					steps: ['Install app', 'Enter model settings', 'Pair browser']
				},
				windows: {
					label: 'Windows',
					title: 'Graphical installer',
					body: 'Download the latest setup app. It installs the launcher, Start Menu entry, desktop shortcut, curated skills, and setup wizard.',
					primaryLabel: 'Download installer',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: 'The tray launcher starts at login, manages model settings, controls the daemon, and prompts when updates are ready.',
					steps: ['Run setup', 'Use the wizard', 'Pair browser']
				},
				linux: {
					label: 'Linux',
					title: 'Local daemon install',
					body: 'Linux keeps the CLI-first runtime with daemon autostart. The browser side panel still connects to the same local gateway.',
					primaryLabel: 'Copy installer',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'Linux install script',
					note: 'Use this path for workstations, servers, and users who prefer managing the runtime directly.',
					steps: ['Install runtime', 'Configure provider', 'Pair browser']
				}
			}
		},
		browser: {
			badge: 'Browser side panel',
			title: 'The browser gives Anda a body on the web',
			body: 'Ask about the active page, collect evidence, and let the local daemon act through browser tools when you approve.',
			store: 'Add extension',
			docs: 'Pair browser',
			features: [
				{
					title: 'Bring page context into memory',
					detail:
						'Send title, URL, selection, page text, screenshots, structured data, and accessibility context into the local agent.'
				},
				{
					title: 'Act with permission',
					detail:
						'Open tabs, switch pages, click, type, scroll, download, print to PDF, and inspect elements from the same conversation.'
				},
				{
					title: 'Keep the same Brain',
					detail:
						'Browser work connects to the same daemon, files, tools, skills, channels, and long-term Brain memory.'
				}
			]
		},
		launcher: {
			badge: 'Desktop launcher',
			title: 'A resident app for a local Brain',
			body: 'Setup, status, pairing, logs, restart, and updates stay close to the OS so Anda can be used every day.',
			features: [
				{
					title: 'First-run setup',
					detail:
						'Configure provider, API key, model, and home directory without hunting through config files.'
				},
				{
					title: 'Daemon control',
					detail:
						'Open Anda, check status, restart the local daemon, edit model settings, and jump to logs from the menu.'
				},
				{
					title: 'Browser pairing',
					detail:
						'Generate a Gateway URL and Bearer token from the launcher, then paste them into the side panel.'
				},
				{
					title: 'Update prompts',
					detail:
						'Check automatically, download release assets, and install updates with a restart prompt when ready.'
				}
			]
		},
		memory: {
			badge: 'Anda Brain',
			title: 'The Brain is the product',
			body: 'Models are inference interfaces. The durable asset is the local graph of your projects, preferences, relationships, and decisions.',
			features: [
				{
					title: 'Local knowledge graph',
					detail:
						'Brain forms a Cognitive Nexus of people, projects, preferences, decisions, events, and changing facts.'
				},
				{
					title: 'Continuous identity',
					detail:
						'Anda can carry your context, working style, recurring responsibilities, and trusted relationships across sessions.'
				},
				{
					title: 'Cross-model memory',
					detail:
						'The model is replaceable. The memory remains local, inspectable, and independent of one provider account.'
				},
				{
					title: 'Tool-aware context',
					detail:
						'Files, shell tools, scheduled jobs, documents, browser actions, and subagents all feed the same assistant context.'
				}
			]
		},
		work: {
			badge: 'Tradeoff',
			title: 'Not the widest toolbox. The most durable assistant',
			body: 'Anda Bot prioritizes memory quality, outside-world exploration, and daily usability over bundling every possible feature.',
			surfaces: [
				{
					label: 'Memory mechanism',
					detail:
						'Make preference, project, relationship, and decision recall reliable before adding more knobs.'
				},
				{
					label: 'Explore the world',
					detail:
						'Use browser context, documents, files, shell tools, and scheduled tasks to gather evidence.'
				},
				{
					label: 'Daily experience',
					detail:
						'Move from terminal-only workflows into a launcher and side panel normal users can live with.'
				},
				{
					label: 'Open edges',
					detail:
						'Keep skills, tools, subagents, and external coding assistants available without locking memory away.'
				}
			]
		},
		final: {
			title: 'Build around memory you own',
			body: 'Use Codex or Claude Code for focused coding. Let Anda Bot keep the durable assistant layer beside them.',
			install: 'Install app',
			docs: 'Read docs',
			github: 'GitHub'
		}
	},
	zh: {
		meta: {
			title: 'Anda Bot - 记忆优先的本地 AI 助手',
			description:
				'把 Anda Bot 作为记忆优先的本地 AI 助手，让知识图谱、上下文、偏好、工具和长期任务始终掌握在自己手中。',
			ogTitle: 'Anda Bot - 记忆优先的本地 AI 助手',
			ogDescription: '安装桌面启动器，连接浏览器扩展，将长期知识图谱安全保存在本地。'
		},
		nav: {
			install: '获取应用',
			why: '为何选择',
			browser: '浏览器',
			launcher: '启动器',
			memory: '大脑记忆',
			docs: '文档'
		},
		language: { label: '语言' },
		hero: {
			badge: '记忆优先的本地 AI 助手',
			title: '模型更迭不息，记忆始终如一',
			body: 'Anda Bot 将长期知识图谱留在本地机器上。无论平台、模型或会话如何更换，你的专属助手始终都在。',
			primary: '获取应用',
			secondary: '加入扩展'
		},
		proof: [
			{ value: '记忆优先', label: '以本地图谱为核心构建，摆脱单一模型账号的绑定' },
			{ value: '无缝迁移', label: '自由切换模型，无需重新积累上下文与个人偏好' },
			{
				value: '全场景覆盖',
				label: '浏览器、启动器、终端、技能、定时任务及消息频道，共享同一个 Brain'
			}
		],
		why: {
			badge: '为何选择 Anda Bot',
			title: '让代码智能体专注编码，让 Anda Bot 负责长久陪伴',
			body: 'Claude Code 与 Codex 是出色的代码库助手；而 Anda Bot 则是长久运行的助手层，在各种工作中始终牢记你的习惯与偏好。',
			routes: [
				{
					name: 'Claude Code 与 Codex',
					role: '专注的编码工作',
					fit: '适用于以代码库为上下文、任务结束后无需保留个人记忆的场景。'
				},
				{
					name: 'OpenClaw 与 Hermes 类平台',
					role: '海量的工具与插件生态',
					fit: '适用于追求生态广度、开箱即用的技能以及海量现成功能的场景。'
				},
				{
					name: 'Anda Bot',
					role: '个人助手的长期底座',
					fit: '适用于需要在模型更迭中保留个人偏好、关系网络、研究轨迹、日常事务与专属身份记忆的场景。',
					primary: true
				}
			]
		},
		install: {
			badge: '快速开始',
			title: '安装真正掌握记忆的本地应用',
			body: '从启动器开始，连接浏览器，让后台守护进程与 Brain 在本地持续运行。',
			detected: '检测到 {os}',
			chooseOs: '选择操作系统',
			tabAria: '各操作系统的安装路径',
			copy: '复制',
			copied: '已复制',
			copyFailed: '复制失败',
			copyAria: '复制安装命令',
			commandAria: '点击复制安装命令',
			options: {
				macos: {
					label: 'macOS',
					title: '菜单栏启动器',
					body: '安装脚本会自动添加 Anda Bot.app，将其设为登录时启动的菜单栏应用，并在设置完成后启动守护进程。',
					primaryLabel: '复制安装命令',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'macOS 安装脚本',
					note: '启动器支持检查更新、重启守护进程、查看日志，并能快速生成浏览器配对 Token。',
					steps: ['安装应用', '配置模型', '连接浏览器']
				},
				windows: {
					label: 'Windows',
					title: '图形化安装程序',
					body: '下载最新的安装程序。它会自动布置启动器、开始菜单、桌面快捷方式、精选技能以及设置向导。',
					primaryLabel: '下载安装程序',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: '系统托盘启动器会随开机自动运行，方便你管理模型设置、控制守护进程，并接收更新推送。',
					steps: ['运行安装', '跟随向导', '连接浏览器']
				},
				linux: {
					label: 'Linux',
					title: '本地守护进程',
					body: 'Linux 版本保留了命令行优先的运行模式和开机自启的守护进程。浏览器侧边栏同样可连接至该本地网关。',
					primaryLabel: '复制安装命令',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'Linux 安装脚本',
					note: '推荐工作站、服务器用户及偏好直接管理运行环境的极客使用此方式。',
					steps: ['环境部署', '配置服务商', '连接浏览器']
				}
			}
		},
		browser: {
			badge: '浏览器侧边栏',
			title: '让 Anda 触及 Web 世界',
			body: '向它询问当前页面的相关内容，收集有效信息，并在你的授权下让本地守护进程通过浏览器工具执行操作。',
			store: '添加扩展',
			docs: '连接浏览器',
			features: [
				{
					title: '将页面上下文纳入记忆',
					detail:
						'一键将标题、URL、选中文本、网页全文、截图、结构化数据及无障碍上下文传递给本地智能体。'
				},
				{
					title: '在授权下执行交互',
					detail:
						'可在同一对话中打开及切换标签页、点击、输入、滚动、下载、打印 PDF 并审查网页元素。'
				},
				{
					title: '连接同一个 Brain',
					detail:
						'浏览器端的操作依然连接着相同的守护进程、系统文件、工作技能、消息频道与长期图谱记忆。'
				}
			]
		},
		launcher: {
			badge: '桌面启动器',
			title: '本地 Brain 的极简常驻入口',
			body: '设置、状态查看、配对、日志、重启及系统更新都与操作系统深度融合，让 Anda 成为得心应手的日常助手。',
			features: [
				{
					title: '直观的可视化配置',
					detail: '无需查阅配置文件，通过界面即可直观配置服务商、API Key、大模型及运行目录。'
				},
				{
					title: '一站式进程控制',
					detail:
						'从菜单快速呼出 Anda，查看运行状态、重启本地进程、编辑模型设置，并一键直达日志文件。'
				},
				{
					title: '连接浏览器面板',
					detail: '通过启动器一键生成网关 URL 和身份验证 Token，将其粘贴至扩展侧边栏即可完成配对。'
				},
				{
					title: '贴心的版本迭代',
					detail: '自动检查新版本，静默下载发布包，并在准备就绪时通过重启提示完成无缝升级。'
				}
			]
		},
		memory: {
			badge: 'Anda Brain',
			title: 'Brain 老伙计才是产品的灵魂',
			body: '模型只是推理的引擎。真正历久弥新的资产，是你本地保存的关于项目、偏好、人物关系与历史决策的知识图谱。',
			features: [
				{
					title: '私有的知识图谱',
					detail: 'Brain 会逐渐交织形成一个涵盖人物、项目、偏好、决策、事件与动态事实的认知枢纽。'
				},
				{
					title: '连贯的身份认知',
					detail:
						'Anda 能够在不断的会话中，承载你的专属上下文、工作习惯、长期职责以及值得信赖的人际关系。'
				},
				{
					title: '跨越模型的记忆',
					detail: '大模型可以随时替换，但记忆依然留在本地，清晰透明，不再受制于单一模型厂商。'
				},
				{
					title: '感知全局上下文',
					detail:
						'工作文件、Shell 脚本、定时任务、备忘文档、浏览器操作与子智能体，都共同滋养着这同一个全局底座。'
				}
			]
		},
		work: {
			badge: '克制的哲学',
			title: '不追求做最庞杂的工具箱，只做最长情耐用的助手',
			body: 'Anda Bot 更加注重记忆质量、对外部世界的探索能力以及日常使用的便利性，而非盲目堆砌各种功能。',
			surfaces: [
				{
					label: '打磨记忆机制',
					detail: '先确保对偏好、项目、关系与决策的回想足够可靠，再逐步增加功能旋钮。'
				},
				{
					label: '探索真实世界',
					detail: '善用浏览器上下文、文档、本地文件、Shell 和定时任务来默默收集信息并加以利用。'
				},
				{
					label: '回归日常体验',
					detail: '从纯终端工具演进为体验极佳的启动器和侧边栏，让普通用户也能获得持久的使用价值。'
				},
				{
					label: '保持开放边界',
					detail: '持续兼容外部的技能、工具、子智能体与专门的代码助手，决不将记忆强行封锁在云端。'
				}
			]
		},
		final: {
			title: '围绕你所真正拥有的记忆构建',
			body: '你可以继续使用 Codex 或 Claude Code 专注编码；同时让 Anda Bot 作为你的长效助手，在旁默默提供支持。',
			install: '获取应用',
			docs: '阅读文档',
			github: 'GitHub'
		}
	},
	es: {
		meta: {
			title: 'Anda Bot - Asistente de IA local con prioridad de memoria',
			description:
				'Use Anda Bot como su asistente local con prioridad de memoria que mantiene su memoria en grafo, contexto, preferencias, herramientas y tareas largas bajo su control.',
			ogTitle: 'Anda Bot - Asistente de IA local con prioridad de memoria',
			ogDescription:
				'Instale el lanzador de escritorio, conecte la extensión del navegador y conserve la memoria en grafo a largo plazo en su propia máquina.'
		},
		nav: {
			install: 'Instalar app',
			why: 'Por qué Anda',
			browser: 'Navegador',
			launcher: 'Lanzador',
			memory: 'Memoria',
			docs: 'Docs'
		},
		language: { label: 'Idioma' },
		hero: {
			badge: 'Asistente de IA local con prioridad de memoria',
			title: 'Su modelo puede cambiar. Su memoria no debería',
			body: 'Anda Bot mantiene la memoria en grafo a largo plazo en su propia máquina, por lo que su asistente sobrevive a plataformas, modelos y sesiones.',
			primary: 'Instalar app',
			secondary: 'Agregar extensión'
		},
		proof: [
			{
				value: 'prioridad de memoria',
				label: 'Construido en torno a la memoria en grafo local, no a una única cuenta de modelo'
			},
			{
				value: 'portable',
				label: 'Cambie de modelo sin tener que reconstruir su contexto y preferencias'
			},
			{
				value: 'entornos diarios',
				label:
					'El navegador, el lanzador, la terminal, las habilidades, el cron y los canales de mensajería comparten un solo Brain'
			}
		],
		why: {
			badge: 'Por qué Anda Bot',
			title: 'Use agentes de código para código. Use Anda Bot para la continuidad',
			body: 'Claude Code y Codex son excelentes dentro de un repositorio. Anda Bot es la capa de asistente de larga duración que recuerda quién es usted a lo largo de su trabajo.',
			routes: [
				{
					name: 'Claude Code y Codex',
					role: 'Sesiones de codificación enfocadas',
					fit: 'Ideal cuando el repositorio es el contexto y la memoria personal es opcional una vez que finaliza la tarea.'
				},
				{
					name: 'OpenClaw y plataformas tipo Hermes',
					role: 'Amplia cobertura de herramientas y complementos',
					fit: 'Ideal cuando la prioridad es la amplitud del ecosistema, habilidades empaquetadas y muchas capacidades listas para usar.'
				},
				{
					name: 'Anda Bot',
					role: 'Base de asistente personal',
					fit: 'Ideal cuando sus preferencias, relaciones, rutas de investigación, rutinas e identidad necesitan sobrevivir a los cambios de modelo.',
					primary: true
				}
			]
		},
		install: {
			badge: 'Comenzar',
			title: 'Instale la aplicación que posee la memoria',
			body: 'Comience con el lanzador, conecte el navegador y mantenga el daemon y el Brain ejecutándose localmente.',
			detected: 'Detectado: {os}',
			chooseOs: 'Elegir SO',
			tabAria: 'Ruta de instalación por sistema operativo',
			copy: 'Copiar',
			copied: 'Copiado',
			copyFailed: 'No se pudo copiar',
			copyAria: 'Copiar comando de instalación',
			commandAria: 'Copiar el comando de instalación',
			options: {
				macos: {
					label: 'macOS',
					title: 'Lanzador en la barra de menú',
					body: 'El script de instalación agrega Anda Bot.app, registra el lanzador en la barra de menú al iniciar sesión y arranca el daemon después de la configuración.',
					primaryLabel: 'Copiar instalador',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'Script de instalación de macOS',
					note: 'El lanzador puede buscar actualizaciones, reiniciar el daemon, abrir registros y crear tokens de emparejamiento para el navegador.',
					steps: ['Instalar app', 'Configurar modelo', 'Emparejar navegador']
				},
				windows: {
					label: 'Windows',
					title: 'Instalador gráfico',
					body: 'Descargue la última aplicación de configuración. Instala el lanzador, una entrada en el Menú de inicio, un acceso directo en el escritorio, habilidades seleccionadas y el asistente de configuración.',
					primaryLabel: 'Descargar instalador',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: 'El lanzador en la bandeja del sistema se inicia al iniciar sesión, administra la configuración del modelo, controla el daemon y avisa cuando las actualizaciones están listas.',
					steps: ['Ejecutar setup', 'Usar el asistente', 'Emparejar navegador']
				},
				linux: {
					label: 'Linux',
					title: 'Instalación del daemon local',
					body: 'Linux conserva el runtime CLI-first con inicio automático del daemon. El panel lateral del navegador se conecta a la misma puerta de enlace local.',
					primaryLabel: 'Copiar instalador',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'Script de instalación de Linux',
					note: 'Use esta ruta para estaciones de trabajo, servidores и пользователей, предпочитающих напрямую управлять средой выполнения.',
					steps: ['Instalar runtime', 'Configurar proveedor', 'Emparejar navegador']
				}
			}
		},
		browser: {
			badge: 'Panel lateral del navegador',
			title: 'El navegador le da cuerpo a Anda en la web',
			body: 'Pregunte sobre la página activa, recopile pruebas y permita que el daemon local actúe a través de las herramientas del navegador con su aprobación.',
			store: 'Agregar extensión',
			docs: 'Emparejar navegador',
			features: [
				{
					title: 'Llevar el contexto de la página a la memoria',
					detail:
						'Envíe el título, la URL, la selección, el texto de la página, las capturas de pantalla, los datos estructurados y el contexto de accesibilidad al agente local.'
				},
				{
					title: 'Actuar con permiso',
					detail:
						'Abra pestañas, cambie de página, haga clic, escriba, desplácese, descarga, imprima a PDF e inspeccione elementos desde la misma conversación.'
				},
				{
					title: 'Mantener el mismo Brain',
					detail:
						'El trabajo en el navegador se conecta al mismo daemon, archivos, herramientas, habilidades, canales y memoria Brain a largo plazo.'
				}
			]
		},
		launcher: {
			badge: 'Lanzador de escritorio',
			title: 'Una aplicación residente para un Brain local',
			body: 'La configuración, el estado, el emparejamiento, los registros, el reinicio y las actualizaciones permanecen cerca del sistema operativo para usar Anda todos los días.',
			features: [
				{
					title: 'Configuración en el primer arranque',
					detail:
						'Configure el proveedor, la clave API, el modelo y el directorio de inicio sin tener que buscar en archivos de configuración.'
				},
				{
					title: 'Control del daemon',
					detail:
						'Abra Anda, verifique el estado, reinicie el daemon local, edite la configuración del modelo y acceda a los registros desde el menú.'
				},
				{
					title: 'Emparejamiento de navegador',
					detail:
						'Genere una Gateway URL y un Bearer token desde el lanzador, luego pégalos en el panel lateral de la extensión.'
				},
				{
					title: 'Actualizaciones de versión',
					detail:
						'Busque actualizaciones automáticamente, descargue los recursos de la versión e instálelos con un aviso de reinicio cuando esté listo.'
				}
			]
		},
		memory: {
			badge: 'Anda Brain',
			title: 'El Brain es el producto',
			body: 'Los modelos son interfaces de inferencia. El activo duradero es el grafo local de sus proyectos, preferencias, relaciones y decisiones.',
			features: [
				{
					title: 'Grafo de conocimiento local',
					detail:
						'El Brain forma un nexo cognitivo (Cognitive Nexus) de personas, proyectos, preferencias, decisiones, eventos y hechos cambiantes.'
				},
				{
					title: 'Identidad continua',
					detail:
						'Anda puede llevar su contexto, estilo de trabajo, responsabilidades recurrentes y relaciones de confianza a través de las sesiones.'
				},
				{
					title: 'Memoria entre modelos',
					detail:
						'El modelo es reemplazable. La memoria permanece local, inspeccionable e independiente de una cuenta de proveedor.'
				},
				{
					title: 'Contexto consciente de herramientas',
					detail:
						'Archivos, herramientas de shell, tareas programadas, documentos, acciones del navegador y subagentes alimentan el mismo contexto del asistente.'
				}
			]
		},
		work: {
			badge: 'Compromiso',
			title: 'No la caja de herramientas más grande. El asistente más duradero',
			body: 'Anda Bot prioriza la calidad de la memoria, la exploración del mundo exterior y la usabilidad diaria sobre la acumulación de todas las funciones posibles.',
			surfaces: [
				{
					label: 'Mecanismo de memoria',
					detail:
						'Haga que el recuerdo de preferencias, proyectos, relaciones y decisiones sea confiable antes de agregar más perillas.'
				},
				{
					label: 'Explorar el mundo',
					detail:
						'Use el contexto del navegador, documentos, archivos, herramientas de shell y tareas programadas para recopilar evidencia.'
				},
				{
					label: 'Experiencia diaria',
					detail:
						'Pase de flujos de trabajo exclusivos de la terminal a un lanzador y un panel lateral con los que los usuarios normales puedan convivir.'
				},
				{
					label: 'Bordes abiertos',
					detail:
						'Mantenga disponibles las habilidades, herramientas, subagentes y asistentes de codificación externos sin bloquear la memoria.'
				}
			]
		},
		final: {
			title: 'Construya sobre memoria que usted posee',
			body: 'Use Codex o Claude Code para codificación enfocada. Deje que Anda Bot mantenga la capa de asistente duradera a su lado.',
			install: 'Instalar app',
			docs: 'Leer docs',
			github: 'GitHub'
		}
	},
	fr: {
		meta: {
			title: 'Anda Bot - Assistant IA local avec priorité à la mémoire',
			description:
				'Utilisez Anda Bot comme assistant local avec priorité à la mémoire, en gardant votre mémoire sous forme de graphe, votre contexte, vos préférences, vos outils et vos tâches longues sous votre contrôle.',
			ogTitle: 'Anda Bot - Assistant IA local avec priorité à la mémoire',
			ogDescription:
				'Installez le lanceur de bureau, connectez l’extension du navigateur et conservez votre mémoire graphe à long terme sur votre propre machine.'
		},
		nav: {
			install: 'Installer l’application',
			why: 'Pourquoi Anda',
			browser: 'Navigateur',
			launcher: 'Lanceur',
			memory: 'Mémoire',
			docs: 'Docs'
		},
		language: { label: 'Langue' },
		hero: {
			badge: 'Assistant IA local avec priorité à la mémoire',
			title: 'Votre modèle peut changer. Votre mémoire ne le devrait pas',
			body: 'Anda Bot conserve une mémoire graphe à long terme sur votre machine, afin que votre assistant survive aux changements de plateformes, de modèles et de sessions.',
			primary: 'Installer l’app',
			secondary: 'Ajouter l’extension'
		},
		proof: [
			{
				value: 'mémoire d’abord',
				label: 'Construit autour de la mémoire graphe locale, pas sur un compte de modèle unique'
			},
			{
				value: 'portable',
				label: 'Changez de modèle sans reconstruire votre contexte et vos préférences'
			},
			{
				value: 'interfaces quotidiennes',
				label:
					'Le navigateur, le lanceur, le terminal, les compétences, le cron et les canaux de messagerie partagent un seul Brain'
			}
		],
		why: {
			badge: 'Pourquoi Anda Bot',
			title: 'Des agents de code pour coder. Anda Bot pour la continuité',
			body: 'Claude Code et Codex excellent au sein d’un dépôt de code. Anda Bot est la couche d’assistant durable qui se souvient de vous à travers toutes vos tâches.',
			routes: [
				{
					name: 'Claude Code et Codex',
					role: 'Sessions de codage focalisées',
					fit: 'Idéal lorsque le dépôt constitue le contexte et que la mémoire personnelle est optionnelle après la tâche.'
				},
				{
					name: 'OpenClaw et plateformes de type Hermes',
					role: 'Large couverture d’outils et de plug-ins',
					fit: 'Idéal lorsque la priorité est la diversité de l’écosystème, les compétences packagées et les nombreuses capacités prêtes à l’emploi.'
				},
				{
					name: 'Anda Bot',
					role: 'Socle d’assistant personnel',
					fit: 'Idéal lorsque vos préférences, relations, parcours de recherche, routines et identité doivent survivre aux changements de modèle.',
					primary: true
				}
			]
		},
		install: {
			badge: 'Démarrer',
			title: 'Installez l’application qui possède la mémoire',
			body: 'Commencez par le lanceur, connectez le navigateur et conservez le démon ainsi que le Brain en local.',
			detected: 'Détecté : {os}',
			chooseOs: 'Choisir l’OS',
			tabAria: 'Chemin d’installation selon le système d’exploitation',
			copy: 'Copier',
			copied: 'Copié',
			copyFailed: 'Copie échouée',
			copyAria: 'Copier la commande d’installation',
			commandAria: 'Copier la commande d’installation',
			options: {
				macos: {
					label: 'macOS',
					title: 'Lanceur de la barre de menus',
					body: 'Le script d’installation ajoute Anda Bot.app, enregistre le lanceur dans la barre de menus au démarrage et lance le démon après la configuration.',
					primaryLabel: 'Copier l’installateur',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'Script d’installation macOS',
					note: 'Le lanceur vérifie les mises à jour, redémarre le démon, ouvre les journaux et génère des jetons d’appairage de navigateur.',
					steps: ['Installer l’app', 'Configurer le modèle', 'Appairer le navigateur']
				},
				windows: {
					label: 'Windows',
					title: 'Installateur graphique',
					body: 'Téléchargez l’application d’installation. Elle installe le lanceur, un raccourci dans le menu Démarrer et sur le bureau, des compétences sélectionnées et l’assistant de configuration.',
					primaryLabel: 'Télécharger l’installateur',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: 'Le lanceur de la zone de notification système démarre à la connexion, gère les paramètres de modèle, contrôle le démon et signale les mises à jour.',
					steps: ['Lancer le setup', 'Utiliser l’assistant', 'Appairer le navigateur']
				},
				linux: {
					label: 'Linux',
					title: 'Installation locale du démon',
					body: 'Linux conserve le runtime CLI-first avec démarrage automatique du démon. Le panneau latéral du navigateur se connecte à la même passerelle locale.',
					primaryLabel: 'Copier l’installateur',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'Script d’installation Linux',
					note: 'Utilisez ce chemin pour les stations de travail, les serveurs et les utilisateurs qui préfèrent gérer directement le runtime.',
					steps: ['Installer le runtime', 'Configurer le fournisseur', 'Appairer le navigateur']
				}
			}
		},
		browser: {
			badge: 'Panneau latéral du navigateur',
			title: 'Le navigateur donne un corps web à Anda',
			body: 'Posez des questions sur la page active, rassemblez des preuves et laissez le démon local agir via les outils du navigateur avec votre accord.',
			store: 'Ajouter l’extension',
			docs: 'Appairer le navigateur',
			features: [
				{
					title: 'Intégrer le contexte de la page en mémoire',
					detail:
						'Envoyez le titre, l’URL, la sélection de texte, le texte de la page, les captures d’écran, les données structurées et le contexte d’accessibilité à l’agent local.'
				},
				{
					title: 'Agir avec votre permission',
					detail:
						'Ouvrez des onglets, changez de page, cliquez, saisissez du texte, faites défiler, téléchargez, imprimez en PDF et inspectez des éléments au sein de la même conversation.'
				},
				{
					title: 'Conserver le même Brain',
					detail:
						'Le travail dans le navigateur se connecte au même démon, fichiers, outils, compétences, canaux et mémoire Brain à long terme.'
				}
			]
		},
		launcher: {
			badge: 'Lanceur de bureau',
			title: 'Une application résidente pour un Brain local',
			body: 'La configuration, l’état, l’appairage, les logs, le redémarrage et les mises à jour restent proches du système d’exploitation pour un usage quotidien.',
			features: [
				{
					title: 'Configuration au premier démarrage',
					detail:
						'Configurez le fournisseur, la clé API, le modèle et le répertoire de base sans chercher dans les fichiers de configuration.'
				},
				{
					title: 'Contrôle du démon',
					detail:
						'Ouvrez Anda, vérifiez l’état, redémarrez le démon local, modifiez les paramètres du modèle et accédez aux logs depuis le menu.'
				},
				{
					title: 'Appairage du navigateur',
					detail:
						'Générez une URL de passerelle et un jeton porteur (Bearer token) depuis le lanceur, puis collez-les dans le panneau latéral de l’extension.'
				},
				{
					title: 'Mises à jour automatiques',
					detail:
						'Vérifiez automatiquement, téléchargez les nouvelles versions et installez-les avec une invite de redémarrage une fois prêtes.'
				}
			]
		},
		memory: {
			badge: 'Anda Brain',
			title: 'Le Brain est le produit',
			body: 'Les modèles ne sont que des moteurs d’inférence. L’actif durable est le graphe local de vos projets, préférences, relations et décisions.',
			features: [
				{
					title: 'Graphe de connaissances local',
					detail:
						'Le Brain forme un nexu cognitif (Cognitive Nexus) reliant personnes, projets, préférences, décisions, événements et faits changeants.'
				},
				{
					title: 'Identité continue',
					detail:
						'Anda porte votre contexte, votre style de travail, vos responsabilités récurrentes et vos relations de confiance d’une session à l’autre.'
				},
				{
					title: 'Mémoire multi-modèles',
					detail:
						'Le modèle peut être remplacé. La mémoire reste locale, inspectable et indépendante de tout compte chez un fournisseur unique.'
				},
				{
					title: 'Contexte conscient des outils',
					detail:
						'Fichiers, outils système, tâches planifiées, documents, actions de navigation et sous-agents alimentent tous le même contexte d’assistant.'
				}
			]
		},
		work: {
			badge: 'Compromis',
			title: 'Pas la boîte à outils la plus vaste. L’assistant le plus durable',
			body: 'Anda Bot donne la priorité à la qualité de la mémoire, à l’exploration du monde extérieur et à l’utilité quotidienne plutôt qu’à l’accumulation de fonctionnalités gadgets.',
			surfaces: [
				{
					label: 'Mécanisme de mémoire',
					detail:
						'Rendre fiables le rappel des préférences, des projets, des relations et des décisions avant d’ajouter d’autres réglages.'
				},
				{
					label: 'Explorer le monde',
					detail:
						'Utiliser le contexte du navigateur, les documents, les fichiers, le shell et les tâches planifiées pour rassembler des informations.'
				},
				{
					label: 'Expérience quotidienne',
					detail:
						'Passer d’un flux de travail purement terminal à un lanceur et un panneau latéral avec lesquels les utilisateurs normaux peuvent vivre.'
				},
				{
					label: 'Ouverture sur l’extérieur',
					detail:
						'Conserver l’accès aux compétences, outils, sous-agents et assistants de codage externes sans emprisonner la mémoire.'
				}
			]
		},
		final: {
			title: 'Construisez autour d’une mémoire qui vous appartient',
			body: 'Utilisez Codex ou Claude Code pour le codage focalisé. Laissez Anda Bot s’occuper de la couche d’assistant durable à leurs côtés.',
			install: 'Installer l’application',
			docs: 'Lire la documentation',
			github: 'GitHub'
		}
	},
	ru: {
		meta: {
			title: 'Anda Bot - AI-помощник с приоритетом памяти',
			description:
				'Используйте Anda Bot как локального помощника с приоритетом памяти, где граф знаний, контекст, предпочтения, инструменты и долгосрочные задачи остаются под вашим полным контролем.',
			ogTitle: 'Anda Bot - AI-помощник с приоритетом памяти',
			ogDescription:
				'Установите лаунчер для рабочего стола, подключите расширение для браузера и храните долгосрочную графовую память на собственной машине.'
		},
		nav: {
			install: 'Установить приложение',
			why: 'Зачем Anda',
			browser: 'Браузер',
			launcher: 'Лаунчер',
			memory: 'Память',
			docs: 'Документация'
		},
		language: { label: 'Язык' },
		hero: {
			badge: 'Локальный AI-помощник с приоритетом памяти',
			title: 'Модели меняются. Ваша память должна оставаться',
			body: 'Anda Bot хранит долгосрочную графовую память на вашей машине, благодаря чему ваш помощник успешно переживает смену платформ, моделей и сессий.',
			primary: 'Установить приложение',
			secondary: 'Добавить расширение'
		},
		proof: [
			{
				value: 'приоритет памяти',
				label: 'Построен вокруг локального графа памяти, а не привязан к одному аккаунту модели'
			},
			{
				value: 'портативность',
				label: 'Меняйте модели без необходимости заново настраивать контекст и предпочтения'
			},
			{
				value: 'все интерфейсы',
				label: 'Браузер, лаунчер, терминал, навыки, задачи cron и мессенджеры делят один Brain'
			}
		],
		why: {
			badge: 'Зачем Anda Bot',
			title: 'Кодовые агенты — для кода. Anda Bot — для непрерывности работы',
			body: 'Claude Code и Codex отлично подходят для работы внутри репозитория. Anda Bot — это долговечный слой помощника, который помнит вас в процессе всей работы.',
			routes: [
				{
					name: 'Claude Code и Codex',
					role: 'Фокусированные сессии программирования',
					fit: 'Лучше всего подходят, когда контекстом является репозиторий, а сохранение личной памяти после завершения задачи необязательно.'
				},
				{
					name: 'OpenClaw и платформы типа Hermes',
					role: 'Широкий спектр инструментов и плагинов',
					fit: 'Лучше всего подходят, когда в приоритете ширина экосистемы, готовые пакеты навыков и множество встроенных возможностей.'
				},
				{
					name: 'Anda Bot',
					role: 'Основа персонального помощника',
					fit: 'Лучше всего подходит, когда ваши предпочтения, контакты, история исследований, рутина и личные особенности должны сохраняться при смене моделей.',
					primary: true
				}
			]
		},
		install: {
			badge: 'Начало работы',
			title: 'Установите приложение, владеющее памятью',
			body: 'Начните с установки лаунчера, подключите браузер и запустите фоновую службу вместе с Brain локально на вашем компьютере.',
			detected: 'Обнаружена ОС: {os}',
			chooseOs: 'Выбрать ОС',
			tabAria: 'Варианты установки в зависимости от операционной системы',
			copy: 'Копировать',
			copied: 'Скопировано',
			copyFailed: 'Копирование не удалось',
			copyAria: 'Копировать команду установки',
			commandAria: 'Скопировать команду установки',
			options: {
				macos: {
					label: 'macOS',
					title: 'Лаунчер в строке меню',
					body: 'Скрипт установки добавляет Anda Bot.app, регистрирует лаунчер в строке меню при входе в систему и запускает фоновую службу демона после завершения настройки.',
					primaryLabel: 'Копировать установщик',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'Скрипт установки macOS',
					note: 'Лаунчер позволяет проверять наличие обновлений, перезапускать демона, просматривать логи и создавать токены авторизации для браузера.',
					steps: ['Установить приложение', 'Настроить модель', 'Связать с браузером']
				},
				windows: {
					label: 'Windows',
					title: 'Графический установщик',
					body: 'Скачайте последнюю версию программы установки. Она установит лаунчер, ярлык в меню «Пуск» и на рабочем столе, предустановленные навыки и запустит мастер настройки.',
					primaryLabel: 'Скачать установщик',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: 'Лаунчер в системном трее запускается при входе в систему, управляет настройками моделей, контролирует демона и сообщает о готовности обновлений.',
					steps: ['Запустить установку', 'Использовать мастер', 'Связать с браузером']
				},
				linux: {
					label: 'Linux',
					title: 'Установка локального демона',
					body: 'Версия для Linux сохраняет приоритет интерфейса командной строки с автозапуском демона. Боковая панель браузера по-прежнему подключается к тому же локальному шлюзу.',
					primaryLabel: 'Копировать установщик',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'Скрипт установки Linux',
					note: 'Этот путь подходит для рабочих станций, серверов и пользователей, предпочитающих напрямую управлять средой выполнения.',
					steps: ['Установить среду', 'Настроить провайдера', 'Связать с браузером']
				}
			}
		},
		browser: {
			badge: 'Боковая панель браузера',
			title: 'Браузер дает Anda тело во Всемирной паутине',
			body: 'Задавайте вопросы об активной странице, собирайте факты и позволяйте локальному демону совершать действия через браузерные инструменты с вашего согласия.',
			store: 'Добавить расширение',
			docs: 'Связать с браузером',
			features: [
				{
					title: 'Перенос контекста страницы в память',
					detail:
						'Передавайте заголовок, URL-адрес, выделенный текст, текстовое содержимое страницы, снимки экрана, структурированные данные и контекст специальных возможностей локальному агенту.'
				},
				{
					title: 'Действия под контролем',
					detail:
						'Открывайте вкладки, переключайте страницы, кликайте, вводите текст, прокручивайте, скачивайте файлы, экспортируйте страницы в PDF и инспектируйте элементы в рамках одного диалога.'
				},
				{
					title: 'Единый Brain на все случаи',
					detail:
						'Работа в браузере связывается с тем же локальным демоном, файлами, инструментами, навыками, каналами коммуникации и долгосрочной памятью Brain.'
				}
			]
		},
		launcher: {
			badge: 'Десктопный лаунчер',
			title: 'Фоновое приложение для локального Brain',
			body: 'Настройка, статус, связывание, логи, перезапуск и обновления всегда находятся под рукой в операционной системе для удобного ежедневного использования Anda.',
			features: [
				{
					title: 'Первичная настройка',
					detail:
						'Быстро укажите провайдера, API-ключ, модель и домашний каталог без необходимости вручную искать файлы конфигурации.'
				},
				{
					title: 'Управление демоном',
					detail:
						'Запускайте Anda, проверяйте текущий статус, перезапускайте локального демона, редактируйте настройки моделей и открывайте логи прямо из системного меню.'
				},
				{
					title: 'Связывание с браузером',
					detail:
						'Создавайте Gateway URL и Bearer-токен прямо в лаунчере, а затем просто вставьте их в настройки боковой панели расширения.'
				},
				{
					title: 'Автоматическое обновление',
					detail:
						'Приложение само проверяет наличие обновлений, скачивает новые версии и устанавливает их, предлагая перезапуск при готовности.'
				}
			]
		},
		memory: {
			badge: 'Anda Brain',
			title: 'Brain — это сам продукт',
			body: 'Модели — это лишь интерфейсы для вычислений. Настоящий ценный актив — это локально сохраненный граф ваших проектов, предпочтений, связей и решений.',
			features: [
				{
					title: 'Локальный граф знаний',
					detail:
						'Brain постепенно выстраивает когнитивную связь (Cognitive Nexus) между людьми, проектами, предпочтениями, решениями, событиями и динамическими фактами.'
				},
				{
					title: 'Стабильное самосознание',
					detail:
						'Anda способна переносить ваш рабочий стиль, привычки, регулярные задачи и доверенные контакты из сессии в сессию.'
				},
				{
					title: 'Память поверх разных моделей',
					detail:
						'Вы можете в любой момент сменить модель. Память останется локальной, прозрачной и независимой от аккаунта конкретного провайдера.'
				},
				{
					title: 'Интеграция с инструментами',
					detail:
						'Файлы, консольные утилиты, задачи по расписанию, документы, действия в браузере и субагенты питают один общий контекст помощника.'
				}
			]
		},
		work: {
			badge: 'Разумный компромисс',
			title: 'Не самый перегруженный комбайн, но самый долговечный помощник',
			body: 'Anda Bot ставит качество памяти, исследование внешнего мира и удобство ежедневного использования выше бездумного накопления всех возможных функций.',
			surfaces: [
				{
					label: 'Механизм памяти',
					detail:
						'Мы делаем воспоминания о ваших предпочтениях, проектах, контактах и решениях абсолютно надежными, прежде чем добавлять новые переключатели.'
				},
				{
					label: 'Исследование мира',
					detail:
						'Используйте контекст браузера, документы, файлы, консольные инструменты и задачи по расписанию для сбора информации.'
				},
				{
					label: 'Ежедневный опыт',
					detail:
						'Переход от сценариев работы исключительно через терминал к удобному лаунчеру и боковой панели, подходящим для обычных пользователей.'
				},
				{
					label: 'Открытые границы',
					detail:
						'Сохраняйте доступ к внешним навыкам, инструментам, субагентам и сторонним помощникам программирования без изоляции памяти в облаке.'
				}
			]
		},
		final: {
			title: 'Стройте работу вокруг памяти, которая принадлежит вам',
			body: 'Продолжайте использовать Codex или Claude Code для узконаправленного написания кода. Позвольте Anda Bot служить надежным помощником рядом с ними.',
			install: 'Установить приложение',
			docs: 'Читать документацию',
			github: 'GitHub'
		}
	},
	ar: {
		meta: {
			title: 'Anda Bot - مساعد الذكاء الاصطناعي المحلي القائم على الذاكرة أولاً',
			description:
				'استخدم Anda Bot كمساعد محلي قائم على الذاكرة أولاً، والذي يحافظ على ذاكرة الرسوم البيانية والسياق والتفضيلات والأدوات والمهام الطويلة تحت تحكمك الكامل.',
			ogTitle: 'Anda Bot - مساعد الذكاء الاصطناعي المحلي القائم على الذاكرة أولاً',
			ogDescription:
				'قم بتثبيت مشغل سطح المكتب، وتوصيل إضافة المتصفح، والاحتفاظ بذاكرة الرسوم البيانية طويلة المدى على جهازك الخاص.'
		},
		nav: {
			install: 'تثبيت التطبيق',
			why: 'لماذا Anda',
			browser: 'المتصفح',
			launcher: 'المشغل',
			memory: 'الذاكرة',
			docs: 'الوثائق'
		},
		language: { label: 'اللغة' },
		hero: {
			badge: 'مساعد الذكاء الاصطناعي المحلي القائم على الذاكرة أولاً',
			title: 'يمكن لنموذجك أن يتغير. لكن ذاكرتك لا ينبغي لها ذلك',
			body: 'يحفظ Anda Bot ذاكرة الرسوم البيانية طويلة المدى محلياً على جهازك، بحيث يستمر مساعدك عبر المنصات والنماذج والجلسات المختلفة.',
			primary: 'تثبيت التطبيق',
			secondary: 'إضافة الامتداد'
		},
		proof: [
			{
				value: 'الذاكرة أولاً',
				label: 'مبني حول ذاكرة الرسوم البيانية المحلية، وليس حول حساب نموذج واحد'
			},
			{
				value: 'قابل للنقل',
				label: 'قم بتبديل النماذج دون الحاجة لإعادة بناء السياق وتفضيلاتك الخاصة'
			},
			{
				value: 'واجهات يومية',
				label:
					'يتشارك المتصفح، والمشغل، والطرفية، والمهارات، والمهام المجدولة (cron), وقنوات المراسلة في Brain واحد'
			}
		],
		why: {
			badge: 'لماذا Anda Bot',
			title: 'استخدم وكلاء البرمجة للكود. واستخدم Anda Bot للاستمرارية',
			body: 'يعد Claude Code و Codex ممتازين داخل المستودع. Anda Bot هو طبقة المساعد طويلة المدى التي تتذكر هويتك أثناء العمل.',
			routes: [
				{
					name: 'Claude Code و Codex',
					role: 'جلسات برمجة مركزة',
					fit: 'الأفضل عندما يكون المستودع هو السياق وتكون الذاكرة الشخصية اختيارية بعد انتهاء المهمة.'
				},
				{
					name: 'OpenClaw والمنصات الشبيهة بـ Hermes',
					role: 'تغطية واسعة للأدوات والإضافات',
					fit: 'الأفضل عندما تكون الأولوية لاتساع النظام البيئي، والمهارات الجاهزة، والعديد من القدرات المضمنة.'
				},
				{
					name: 'Anda Bot',
					role: 'ركيزة المساعد الشخصي',
					fit: 'الأفضل عندما تحتاج تفضيلاتك وعلاقاتك ومسارات أبحاثك وروتينك وهويتك للبقاء بعد تغيير النموذج.',
					primary: true
				}
			]
		},
		install: {
			badge: 'ابدأ العمل',
			title: 'ثبّت التطبيق الذي يمتلك الذاكرة',
			body: 'ابدأ باستخدام المشغل، وقم بتوصيل المتصفح، وحافظ على تشغيل الخدمة الخلفية (daemon) وBrain محلياً.',
			detected: 'تم اكتشاف نظام التشغيل: {os}',
			chooseOs: 'اختر نظام التشغيل',
			tabAria: 'مسار التثبيت حسب نظام التشغيل المكتشف',
			copy: 'نسخ',
			copied: 'تم النسخ',
			copyFailed: 'فشل النسخ',
			copyAria: 'نسخ أمر التثبيت',
			commandAria: 'نسخ أمر التثبيت المتاح',
			options: {
				macos: {
					label: 'macOS',
					title: 'مشغل شريط القوائم',
					body: 'يقوم سكريبت التثبيت بإضافة Anda Bot.app، وتسجيل المشغل في شريط القوائم عند تسجيل الدخول، وتشغيل الخدمة الخلفية (daemon) بعد التثبيت.',
					primaryLabel: 'نسخ برنامج التثبيت',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'سكريبت تثبيت macOS',
					note: 'يمكن للمشغل فحص التحديثات، وإعادة تشغيل daemon، وفتح سجلات الأحداث، وإنشاء رموز ربط المتصفح.',
					steps: ['تثبيت التطبيق', 'إعداد النموذج', 'ربط المتصفح']
				},
				windows: {
					label: 'Windows',
					title: 'برنامج تثبيت رسومي',
					body: 'قم بتنزيل أحدث تطبيق إعداد. حيث يقوم بتثبيت المشغل، وإدخال قائمة ابدأ، واختصار سطح المكتب، والمهارات المنسقة، ومعالج الإعداد.',
					primaryLabel: 'تنزيل برنامج التثبيت',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: 'يبدأ مشغل علبة النظام عند تسجيل الدخول، ويدير إعدادات النموذج، ويتحكم في daemon، وينبهك عندما تكون التحديثات جاهزة.',
					steps: ['تشغيل الإعداد', 'استخدام المعالج', 'ربط المتصفح']
				},
				linux: {
					label: 'Linux',
					title: 'تثبيت daemon المحلي',
					body: 'يحافظ نظام Linux على بيئة التشغيل التي تركز على واجهة الأوامر (CLI) مع التشغيل التلقائي لـ daemon. لا تزال اللوحة الجانبية للمتصفح تتصل بنفس البوابة المحلية.',
					primaryLabel: 'نسخ برنامج التثبيت',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'سكريبت تثبيت Linux',
					note: 'استخدم هذا المسار لمحطات العمل والخوادم والمستخدمين الذين يفضلون إدارة بيئة التشغيل مباشرة.',
					steps: ['تثبيت بيئة التشغيل', 'تكوين المزود', 'ربط المتصفح']
				}
			}
		},
		browser: {
			badge: 'اللوحة الجانبية للمتصفح',
			title: 'يمنح المتصفح Anda جسداً على شبكة الويب',
			body: 'اسأل عن الصفحة النشطة، واجمع الأدلة والبيانات، واسمح للـ daemon المحلي بالعمل عبر أدوات المتصفح بعد موافقتك الصريحة.',
			store: 'إضافة الامتداد',
			docs: 'ربط المتصفح',
			features: [
				{
					title: 'إدخال سياق الصفحة في الذاكرة',
					detail:
						'أرسل العنوان والرابط والتحديد ونص الصفحة ولقطات الشاشة والبيانات المنظمة وسياق إمكانية الوصول إلى الوكيل المحلي.'
				},
				{
					title: 'العمل بموافقتك',
					detail:
						'افتح التبويبات، وبدّل الصفحات، وانقر، واكتب، ومرر، ونزّل الملفات، واطبع إلى PDF، وافحص العناصر داخل المحادثة نفسها.'
				},
				{
					title: 'الاحتفاظ بنفس الـ Brain',
					detail:
						'يتصل عمل المتصفح بنفس daemon والملفات والأدوات والمهارات والقنوات وذاكرة Brain طويلة المدى.'
				}
			]
		},
		launcher: {
			badge: 'مشغل سطح المكتب',
			title: 'تطبيق مقيم لـ Brain المحلي',
			body: 'تظل إعدادات التهيئة، والحالة، والربط، والسجلات، وإعادة التشغيل، والتحديثات قريبة من نظام التشغيل لاستخدام Anda يومياً.',
			features: [
				{
					title: 'التهيئة عند التشغيل الأول',
					detail:
						'قم بتكوين المزود، ومفتاح API، والنموذج، والدليل الرئيسي دون الحاجة للبحث في ملفات التكوين المعقدة.'
				},
				{
					title: 'التحكم في الـ daemon',
					detail:
						'افتح Anda، وافحص الحالة، وأعد تشغيل daemon المحلي، وعدل إعدادات النموذج، وانتقل إلى السجلات من القائمة.'
				},
				{
					title: 'ربط المتصفح',
					detail:
						'قم بإنشاء Gateway URL و Bearer token من المشغل، ثم الصقهما في اللوحة الجانبية للامتداد.'
				},
				{
					title: 'تحديثات الإصدارات',
					detail:
						'افحص التحديثات تلقائياً، ونزّل أصول الإصدار، وثبت التحديثات مع إشعار بإعادة التشغيل عند الجاهزية.'
				}
			]
		},
		memory: {
			badge: 'Anda Brain',
			title: 'Brain هو جوهر المنتج الحقيقي',
			body: 'النماذج هي مجرد واجهات استدلال. الأصول الدائمة والمستقرة هي الرسم البياني المحلي لمشاريعك وتفضيلاتك وعلاقاتك وقراراتك.',
			features: [
				{
					title: 'مخطط المعرفة المحلي',
					detail:
						'يشكل Brain محوراً معرفياً (Cognitive Nexus) يربط الأشخاص والمشاريع والتفضيلات والقرارات والأحداث والحقائق المتغيرة.'
				},
				{
					title: 'الهوية المستمرة',
					detail:
						'يمكن لـ Anda نقل سياقك، وأسلوب عملك، ومسؤولياتك المتكررة، وعلاقاتك الموثوقة عبر الجلسات المختلفة.'
				},
				{
					title: 'الذاكرة العابرة للنماذج',
					detail:
						'النموذج قابل للاستبدال والترقية. بينما تظل الذاكرة محلية وقابلة للفحص ومستقلة تماماً عن حساب أي مزود منفرد.'
				},
				{
					title: 'سياق واعٍ بالأدوات',
					detail:
						'تغذي الملفات، وأدوات القشرة (shell)، والوظائف المجدولة، والمستندات، وإجراءات المتصفح، والوكلاء الفرعيون نفس سياق المساعد.'
				}
			]
		},
		work: {
			badge: 'المفاضلة والالتزام',
			title: 'ليس صندوق الأدوات الأكبر، بل المساعد الأطول عمراً والأكثر استقراراً',
			body: 'يعطي Anda Bot الأولوية لجودة الذاكرة، واستكشاف العالم الخارجي، وسهولة الاستخدام اليومي على حساب تكديس الميزات غير الضرورية.',
			surfaces: [
				{
					label: 'آلية الذاكرة',
					detail:
						'تأكد من جعل استدعاء التفضيلات والمشاريع والعلاقات والقرارات موثوقاً قبل إضافة المزيد من المفاتيح والأزرار.'
				},
				{
					label: 'استكشاف العالم',
					detail:
						'استخدم سياق المتصفح، والمستندات، والملفات، وأدوات shell، والمهام المجدولة لجمع الأدلة والمعلومات.'
				},
				{
					label: 'تجربة الاستخدام اليومية',
					detail:
						'انتقل من سير العمل المقتصر على الطرفية فقط إلى مشغل لوحة جانبية يستطيع المستخدمون العاديون التعايش معها يومياً.'
				},
				{
					label: 'حدود مفتوحة',
					detail:
						'حافظ على توفر المهارات والأدوات والوكلاء الفرعيين ومساعدي البرمجة الخارجيين دون حبس الذاكرة في مكان مغلق.'
				}
			]
		},
		final: {
			title: 'ابنِ حول ذاكرة تمتلكها بنفسك',
			body: 'استخدم Codex أو Claude Code للبرمجة المركزة. ودع Anda Bot يحفظ طبقة المساعد المستقرة بجانبهما.',
			install: 'تثبيت التطبيق',
			docs: 'قراءة الوثائق',
			github: 'GitHub'
		}
	}
};

export function isLocale(value: string | null | undefined): value is Locale {
	return Boolean(value && value in localeMeta);
}

export function detectLocale(languages: readonly string[]): Locale {
	for (const language of languages) {
		const tag = language.toLowerCase();
		const base = tag.split('-')[0];
		if (isLocale(base)) return base;
	}

	return fallbackLocale;
}
