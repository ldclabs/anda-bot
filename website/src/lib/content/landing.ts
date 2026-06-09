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
			title: 'Anda Bot - Asistente IA local con memoria primero',
			description:
				'Usa Anda Bot como asistente local con memoria primero, manteniendo memoria en grafo, contexto, preferencias, herramientas y tareas largas bajo tu control.',
			ogTitle: 'Anda Bot - Asistente IA local con memoria primero',
			ogDescription:
				'Instala el launcher, conecta la extensión del navegador y conserva la memoria en grafo en tu propia máquina.'
		},
		nav: {
			install: 'Instalar app',
			why: 'Por qué',
			browser: 'Navegador',
			launcher: 'Launcher',
			memory: 'Memoria',
			docs: 'Docs'
		},
		language: { label: 'Idioma' },
		hero: {
			badge: 'Asistente local con memoria primero',
			title: 'Puedes cambiar de modelo. Tu memoria no debería perderse',
			body: 'Anda Bot guarda memoria en grafo en tu máquina para sobrevivir a plataformas, modelos y sesiones.',
			primary: 'Instalar app',
			secondary: 'Agregar extensión'
		},
		proof: [
			{
				value: 'memory-first',
				label: 'Construido alrededor de memoria local, no de una cuenta de modelo'
			},
			{
				value: 'portable',
				label: 'Cambia modelos sin reconstruir tu contexto y preferencias'
			},
			{
				value: 'superficies',
				label: 'Navegador, launcher, terminal, skills, cron e IM comparten un Brain'
			}
		],
		why: {
			badge: 'Por qué Anda Bot',
			title: 'Agentes de código para código. Anda Bot para continuidad',
			body: 'Claude Code y Codex son excelentes dentro de un repo. Anda Bot es la capa de asistente que recuerda quién eres.',
			routes: [
				{
					name: 'Claude Code y Codex',
					role: 'Sesiones de código enfocadas',
					fit: 'Mejor cuando el repositorio es el contexto y la memoria personal no es necesaria al terminar.'
				},
				{
					name: 'OpenClaw y plataformas tipo Hermes',
					role: 'Herramientas y plugins amplios',
					fit: 'Mejor cuando importan ecosistema, skills empaquetadas y muchas capacidades listas.'
				},
				{
					name: 'Anda Bot',
					role: 'Base de asistente personal',
					fit: 'Mejor cuando preferencias, relaciones, investigación, rutinas e identidad deben sobrevivir al cambio de modelo.',
					primary: true
				}
			]
		},
		install: {
			badge: 'Comienza',
			title: 'Instala la app que posee la memoria',
			body: 'Empieza con el launcher, conecta el navegador y mantén daemon y Brain corriendo localmente.',
			detected: '{os} detectado',
			chooseOs: 'Elige SO',
			tabAria: 'Ruta de instalación por sistema operativo',
			copy: 'Copiar',
			copied: 'Copiado',
			copyFailed: 'No se pudo copiar',
			copyAria: 'Copiar comando de instalación',
			commandAria: 'Copiar el comando de instalación',
			options: {
				macos: {
					label: 'macOS',
					title: 'Launcher de menú',
					body: 'El script instala Anda Bot.app, registra el launcher al iniciar sesión y arranca el daemon después de configurar.',
					primaryLabel: 'Copiar instalador',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'Script de macOS',
					note: 'El launcher revisa actualizaciones, reinicia el daemon, abre logs y crea tokens para el navegador.',
					steps: ['Instalar app', 'Configurar modelo', 'Conectar navegador']
				},
				windows: {
					label: 'Windows',
					title: 'Instalador gráfico',
					body: 'Descarga la app de setup. Instala launcher, accesos, skills seleccionadas y asistente de configuración.',
					primaryLabel: 'Descargar instalador',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: 'El launcher de bandeja arranca con la sesión, controla el daemon y avisa cuando una actualización está lista.',
					steps: ['Ejecutar setup', 'Usar asistente', 'Conectar navegador']
				},
				linux: {
					label: 'Linux',
					title: 'Instalación de daemon local',
					body: 'Linux mantiene el runtime CLI-first con autostart del daemon. El panel lateral se conecta al mismo gateway local.',
					primaryLabel: 'Copiar instalador',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'Script de Linux',
					note: 'Usa esta ruta para estaciones, servidores y personas que prefieren gestionar el runtime directamente.',
					steps: ['Instalar runtime', 'Configurar proveedor', 'Conectar navegador']
				}
			}
		},
		browser: {
			badge: 'Panel lateral',
			title: 'El navegador le da cuerpo a Anda en la web',
			body: 'Pregunta por la página activa, reúne evidencia y deja que el daemon local actúe con tu aprobación.',
			store: 'Agregar extensión',
			docs: 'Conectar navegador',
			features: [
				{
					title: 'Llevar contexto a la memoria',
					detail:
						'Envía título, URL, selección, texto, capturas, datos estructurados y contexto de accesibilidad al agente local.'
				},
				{
					title: 'Actuar con permiso',
					detail:
						'Abre pestañas, cambia páginas, hace clic, escribe, desplaza, descarga, imprime PDF e inspecciona elementos.'
				},
				{
					title: 'Usar el mismo Brain',
					detail:
						'El trabajo del navegador conecta al mismo daemon, archivos, herramientas, skills, canales y memoria Brain.'
				}
			]
		},
		launcher: {
			badge: 'Launcher de escritorio',
			title: 'Una app residente para un Brain local',
			body: 'Setup, estado, pairing, logs, reinicio y updates quedan cerca del sistema para usar Anda cada día.',
			features: [
				{
					title: 'Primer arranque',
					detail: 'Configura proveedor, API key, modelo y home sin buscar archivos.'
				},
				{
					title: 'Control del daemon',
					detail: 'Abre Anda, revisa estado, reinicia el daemon, edita modelo y abre logs.'
				},
				{
					title: 'Emparejar navegador',
					detail: 'Genera Gateway URL y Bearer token, luego pégalos en el panel lateral.'
				},
				{
					title: 'Avisos de actualización',
					detail: 'Revisa, descarga e instala actualizaciones con aviso de reinicio.'
				}
			]
		},
		memory: {
			badge: 'Anda Brain',
			title: 'El Brain es el producto',
			body: 'Los modelos son interfaces de inferencia. El activo duradero es el grafo local de proyectos, preferencias, relaciones y decisiones.',
			features: [
				{
					title: 'Grafo local de conocimiento',
					detail:
						'Brain forma un Cognitive Nexus de personas, proyectos, preferencias, decisiones, eventos y hechos cambiantes.'
				},
				{
					title: 'Identidad continua',
					detail:
						'Anda conserva contexto, estilo de trabajo, responsabilidades y relaciones de confianza entre sesiones.'
				},
				{
					title: 'Memoria entre modelos',
					detail:
						'El modelo se puede reemplazar. La memoria sigue local, inspeccionable e independiente de un proveedor.'
				},
				{
					title: 'Contexto con herramientas',
					detail:
						'Archivos, shell, cron, documentos, navegador y subagents alimentan el mismo contexto.'
				}
			]
		},
		work: {
			badge: 'Tradeoff',
			title: 'No la caja más grande. El asistente más duradero',
			body: 'Anda Bot prioriza memoria, exploración del mundo y uso diario antes que empaquetar toda función posible.',
			surfaces: [
				{
					label: 'Memoria',
					detail:
						'Hacer fiable el recuerdo de preferencias, proyectos, relaciones y decisiones antes de añadir más controles.'
				},
				{
					label: 'Explorar el mundo',
					detail:
						'Usar navegador, documentos, archivos, shell y tareas programadas para reunir evidencia.'
				},
				{
					label: 'Experiencia diaria',
					detail: 'Pasar de flujos solo de terminal a launcher y panel lateral para uso continuo.'
				},
				{
					label: 'Bordes abiertos',
					detail:
						'Mantener skills, herramientas, subagents y asistentes de código sin encerrar la memoria.'
				}
			]
		},
		final: {
			title: 'Construye sobre memoria propia',
			body: 'Usa Codex o Claude Code para código enfocado. Deja que Anda Bot mantenga la capa duradera.',
			install: 'Instalar app',
			docs: 'Leer docs',
			github: 'GitHub'
		}
	},
	fr: {
		meta: {
			title: 'Anda Bot - Assistant IA local centré mémoire',
			description:
				'Utilisez Anda Bot comme assistant local centré mémoire, avec mémoire graphe, contexte, préférences, outils et tâches longues sous votre contrôle.',
			ogTitle: 'Anda Bot - Assistant IA local centré mémoire',
			ogDescription:
				'Installez le lanceur, connectez l extension navigateur et gardez la mémoire graphe sur votre machine.'
		},
		nav: {
			install: 'Installer app',
			why: 'Pourquoi',
			browser: 'Navigateur',
			launcher: 'Lanceur',
			memory: 'Mémoire',
			docs: 'Docs'
		},
		language: { label: 'Langue' },
		hero: {
			badge: 'Assistant local centré mémoire',
			title: 'Le modèle peut changer. Votre mémoire ne devrait pas disparaître',
			body: 'Anda Bot garde une mémoire graphe locale pour survivre aux plateformes, modèles et sessions.',
			primary: 'Installer app',
			secondary: 'Ajouter extension'
		},
		proof: [
			{
				value: 'memory-first',
				label: 'Construit autour de la mémoire locale, pas un compte modèle'
			},
			{
				value: 'portable',
				label: 'Changez de modèle sans reconstruire contexte et préférences'
			},
			{
				value: 'surfaces',
				label: 'Navigateur, lanceur, terminal, skills, cron et IM partagent un Brain'
			}
		],
		why: {
			badge: 'Pourquoi Anda Bot',
			title: 'Agents de code pour le code. Anda Bot pour la continuité',
			body: 'Claude Code et Codex excellent dans un dépôt. Anda Bot est la couche assistant durable qui se souvient de vous.',
			routes: [
				{
					name: 'Claude Code et Codex',
					role: 'Sessions de code focalisées',
					fit: 'Idéal quand le dépôt est le contexte et que la mémoire personnelle est optionnelle après la tâche.'
				},
				{
					name: 'OpenClaw et plateformes type Hermes',
					role: 'Large couverture outils et plugins',
					fit: 'Idéal quand la priorité est l écosystème, les skills packagées et beaucoup de capacités prêtes.'
				},
				{
					name: 'Anda Bot',
					role: 'Socle assistant personnel',
					fit: 'Idéal quand préférences, relations, recherches, routines et identité doivent survivre aux modèles.',
					primary: true
				}
			]
		},
		install: {
			badge: 'Démarrer',
			title: 'Installez l app qui possède la mémoire',
			body: 'Commencez par le lanceur, connectez le navigateur et gardez daemon et Brain en local.',
			detected: '{os} détecté',
			chooseOs: 'Choisir OS',
			tabAria: 'Chemin d installation par système',
			copy: 'Copier',
			copied: 'Copié',
			copyFailed: 'Copie échouée',
			copyAria: 'Copier la commande',
			commandAria: 'Copier la commande d installation',
			options: {
				macos: {
					label: 'macOS',
					title: 'Lanceur de menu',
					body: 'Le script installe Anda Bot.app, enregistre le lanceur au login et démarre le daemon après setup.',
					primaryLabel: 'Copier installateur',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'Script macOS',
					note: 'Le lanceur vérifie les mises à jour, redémarre le daemon, ouvre les logs et crée les tokens navigateur.',
					steps: ['Installer app', 'Configurer modèle', 'Connecter navigateur']
				},
				windows: {
					label: 'Windows',
					title: 'Installateur graphique',
					body: 'Téléchargez le setup. Il installe lanceur, raccourcis, skills sélectionnées et assistant de configuration.',
					primaryLabel: 'Télécharger',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: 'Le lanceur de barre système démarre au login, contrôle le daemon et signale les mises à jour prêtes.',
					steps: ['Lancer setup', 'Utiliser assistant', 'Connecter navigateur']
				},
				linux: {
					label: 'Linux',
					title: 'Installation daemon local',
					body: 'Linux garde le runtime CLI-first avec autostart daemon. Le panneau latéral se connecte au même gateway local.',
					primaryLabel: 'Copier installateur',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'Script Linux',
					note: 'Utilisez ce chemin pour postes, serveurs et personnes qui préfèrent gérer le runtime directement.',
					steps: ['Installer runtime', 'Configurer provider', 'Connecter navigateur']
				}
			}
		},
		browser: {
			badge: 'Panneau navigateur',
			title: 'Le navigateur donne un corps web à Anda',
			body: 'Interrogez la page active, rassemblez les preuves et laissez le daemon agir avec votre accord.',
			store: 'Ajouter extension',
			docs: 'Connecter navigateur',
			features: [
				{
					title: 'Porter le contexte en mémoire',
					detail:
						'Envoyez titre, URL, sélection, texte, captures, données structurées et accessibilité à l agent local.'
				},
				{
					title: 'Agir avec permission',
					detail:
						'Ouvrez des onglets, changez de page, cliquez, saisissez, défilez, téléchargez, imprimez PDF et inspectez.'
				},
				{
					title: 'Garder le même Brain',
					detail:
						'Le travail navigateur rejoint le même daemon, fichiers, outils, skills, canaux et mémoire Brain.'
				}
			]
		},
		launcher: {
			badge: 'Lanceur desktop',
			title: 'Une app résidente pour un Brain local',
			body: 'Setup, statut, appairage, logs, redémarrage et mises à jour restent près du système pour un usage quotidien.',
			features: [
				{
					title: 'Premier setup',
					detail: 'Configurez provider, API key, modèle et home sans chercher les fichiers.'
				},
				{
					title: 'Contrôle daemon',
					detail:
						'Ouvrez Anda, vérifiez le statut, redémarrez le daemon, modifiez le modèle et ouvrez les logs.'
				},
				{
					title: 'Appairage navigateur',
					detail: 'Générez Gateway URL et Bearer token, puis collez-les dans le panneau.'
				},
				{
					title: 'Mises à jour',
					detail: 'Vérifiez, téléchargez et installez avec une invite de redémarrage.'
				}
			]
		},
		memory: {
			badge: 'Anda Brain',
			title: 'Le Brain est le produit',
			body: 'Les modèles sont des interfaces d inférence. L actif durable est le graphe local de projets, préférences, relations et décisions.',
			features: [
				{
					title: 'Graphe local de connaissance',
					detail:
						'Brain forme un Cognitive Nexus de personnes, projets, préférences, décisions, événements et faits changeants.'
				},
				{
					title: 'Identité continue',
					detail:
						'Anda porte votre contexte, style de travail, responsabilités et relations de confiance entre sessions.'
				},
				{
					title: 'Mémoire multi-modèles',
					detail:
						'Le modèle peut être remplacé. La mémoire reste locale, inspectable et indépendante d un fournisseur.'
				},
				{
					title: 'Contexte outillé',
					detail:
						'Fichiers, shell, tâches planifiées, documents, navigateur et subagents nourrissent le même contexte.'
				}
			]
		},
		work: {
			badge: 'Compromis',
			title: 'Pas la boîte la plus large. L assistant le plus durable',
			body: 'Anda Bot priorise mémoire, exploration du monde et usage quotidien avant d intégrer toute fonction possible.',
			surfaces: [
				{
					label: 'Mémoire',
					detail:
						'Rendre fiables préférences, projets, relations et décisions avant d ajouter plus de réglages.'
				},
				{
					label: 'Explorer le monde',
					detail:
						'Utiliser navigateur, documents, fichiers, shell et tâches planifiées pour rassembler des preuves.'
				},
				{
					label: 'Usage quotidien',
					detail: 'Passer du terminal seul à un lanceur et un panneau latéral habitables.'
				},
				{
					label: 'Bords ouverts',
					detail: 'Garder skills, outils, subagents et assistants code sans enfermer la mémoire.'
				}
			]
		},
		final: {
			title: 'Construisez sur une mémoire qui vous appartient',
			body: 'Utilisez Codex ou Claude Code pour coder. Laissez Anda Bot garder la couche assistant durable.',
			install: 'Installer app',
			docs: 'Lire docs',
			github: 'GitHub'
		}
	},
	ru: {
		meta: {
			title: 'Anda Bot - Локальный AI помощник с памятью',
			description:
				'Используйте Anda Bot как локального помощника с памятью, где граф памяти, контекст, предпочтения, tools и длинные задачи остаются под вашим контролем.',
			ogTitle: 'Anda Bot - Локальный AI помощник с памятью',
			ogDescription:
				'Установите launcher, подключите расширение браузера и храните долгую графовую память на своей машине.'
		},
		nav: {
			install: 'Установить',
			why: 'Зачем',
			browser: 'Браузер',
			launcher: 'Launcher',
			memory: 'Память',
			docs: 'Документы'
		},
		language: { label: 'Язык' },
		hero: {
			badge: 'Локальный помощник с памятью',
			title: 'Модель можно сменить. Память не должна исчезнуть',
			body: 'Anda Bot хранит графовую память локально, чтобы помощник переживал платформы, модели и сессии.',
			primary: 'Установить',
			secondary: 'Добавить расширение'
		},
		proof: [
			{ value: 'memory-first', label: 'Построен вокруг локальной памяти, а не аккаунта модели' },
			{ value: 'portable', label: 'Меняйте модели без пересборки контекста и предпочтений' },
			{
				value: 'surfaces',
				label: 'Браузер, launcher, terminal, skills, cron и IM делят один Brain'
			}
		],
		why: {
			badge: 'Зачем Anda Bot',
			title: 'Кодовые агенты для кода. Anda Bot для непрерывности',
			body: 'Claude Code и Codex сильны внутри репозитория. Anda Bot это долговечный слой помощника, который помнит вас.',
			routes: [
				{
					name: 'Claude Code и Codex',
					role: 'Фокусные сессии кодинга',
					fit: 'Лучше всего, когда репозиторий является контекстом, а личная память после задачи необязательна.'
				},
				{
					name: 'OpenClaw и платформы типа Hermes',
					role: 'Широкие tools и plugins',
					fit: 'Лучше всего, когда важны экосистема, готовые skills и много встроенных возможностей.'
				},
				{
					name: 'Anda Bot',
					role: 'Основа личного помощника',
					fit: 'Лучше всего, когда предпочтения, связи, исследования, рутины и идентичность должны пережить смену модели.',
					primary: true
				}
			]
		},
		install: {
			badge: 'Начало',
			title: 'Установите приложение, которому принадлежит память',
			body: 'Начните с launcher, подключите браузер и держите daemon плюс Brain локально.',
			detected: 'Определено: {os}',
			chooseOs: 'Выберите ОС',
			tabAria: 'Путь установки по операционной системе',
			copy: 'Копировать',
			copied: 'Скопировано',
			copyFailed: 'Не скопировано',
			copyAria: 'Копировать команду установки',
			commandAria: 'Копировать команду установки',
			options: {
				macos: {
					label: 'macOS',
					title: 'Menu-bar launcher',
					body: 'Скрипт добавляет Anda Bot.app, регистрирует launcher при входе и запускает daemon после настройки.',
					primaryLabel: 'Копировать установщик',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'Скрипт macOS',
					note: 'Launcher проверяет обновления, перезапускает daemon, открывает логи и создает browser tokens.',
					steps: ['Установить app', 'Настроить модель', 'Подключить браузер']
				},
				windows: {
					label: 'Windows',
					title: 'Графический установщик',
					body: 'Скачайте setup app. Он устанавливает launcher, shortcuts, curated skills и мастер настройки.',
					primaryLabel: 'Скачать установщик',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: 'Tray launcher стартует при входе, управляет daemon и сообщает, когда обновление готово.',
					steps: ['Запустить setup', 'Пройти мастер', 'Подключить браузер']
				},
				linux: {
					label: 'Linux',
					title: 'Установка локального daemon',
					body: 'Linux сохраняет CLI-first runtime с autostart daemon. Боковая панель подключается к тому же local gateway.',
					primaryLabel: 'Копировать установщик',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'Скрипт Linux',
					note: 'Этот путь подходит для рабочих станций, серверов и прямого управления runtime.',
					steps: ['Установить runtime', 'Настроить provider', 'Подключить браузер']
				}
			}
		},
		browser: {
			badge: 'Боковая панель браузера',
			title: 'Браузер дает Anda тело в вебе',
			body: 'Спрашивайте об активной странице, собирайте факты и разрешайте локальному daemon действовать через браузер.',
			store: 'Добавить расширение',
			docs: 'Подключить браузер',
			features: [
				{
					title: 'Передать контекст в память',
					detail:
						'Передайте title, URL, selection, text, screenshots, structured data и accessibility context локальному агенту.'
				},
				{
					title: 'Действовать с разрешением',
					detail:
						'Открывайте tabs, переключайте pages, click, type, scroll, download, print to PDF и inspect elements.'
				},
				{
					title: 'Держать один Brain',
					detail:
						'Работа в браузере подключена к тому же daemon, files, tools, skills, channels и Brain memory.'
				}
			]
		},
		launcher: {
			badge: 'Desktop launcher',
			title: 'Резидентное приложение для локального Brain',
			body: 'Setup, status, pairing, logs, restart и updates остаются рядом с ОС для ежедневного использования Anda.',
			features: [
				{
					title: 'Первый запуск',
					detail: 'Настройте provider, API key, model и home без поиска файлов конфигурации.'
				},
				{
					title: 'Daemon control',
					detail:
						'Откройте Anda, проверьте status, перезапустите daemon, измените model settings и откройте logs.'
				},
				{
					title: 'Browser pairing',
					detail: 'Создайте Gateway URL и Bearer token, затем вставьте их в боковую панель.'
				},
				{
					title: 'Update prompts',
					detail: 'Проверяйте, скачивайте и устанавливайте updates с prompt на restart.'
				}
			]
		},
		memory: {
			badge: 'Anda Brain',
			title: 'Brain это сам продукт',
			body: 'Модели являются интерфейсами вывода. Долговечный актив это локальный граф проектов, предпочтений, связей и решений.',
			features: [
				{
					title: 'Локальный граф знаний',
					detail:
						'Brain формирует Cognitive Nexus из людей, проектов, preferences, decisions, events и меняющихся facts.'
				},
				{
					title: 'Непрерывная идентичность',
					detail:
						'Anda переносит контекст, стиль работы, повторяющиеся обязанности и доверенные связи между сессиями.'
				},
				{
					title: 'Память между моделями',
					detail:
						'Модель заменяема. Память остается локальной, проверяемой и независимой от одного провайдера.'
				},
				{
					title: 'Контекст с tools',
					detail:
						'Files, shell, scheduled jobs, documents, browser actions и subagents питают один контекст.'
				}
			]
		},
		work: {
			badge: 'Компромисс',
			title: 'Не самый широкий набор tools. Самый долговечный помощник',
			body: 'Anda Bot ставит качество памяти, исследование внешнего мира и ежедневный опыт выше упаковки всех функций сразу.',
			surfaces: [
				{
					label: 'Механизм памяти',
					detail: 'Сначала сделать надежным recall предпочтений, проектов, связей и решений.'
				},
				{
					label: 'Исследовать мир',
					detail:
						'Использовать браузер, документы, файлы, shell и scheduled tasks для сбора фактов.'
				},
				{
					label: 'Ежедневный опыт',
					detail:
						'Перейти от terminal-only workflows к launcher и side panel для долгого использования.'
				},
				{
					label: 'Открытые края',
					detail:
						'Сохранить skills, tools, subagents и external coding assistants без запирания памяти.'
				}
			]
		},
		final: {
			title: 'Стройте вокруг памяти, которой владеете.',
			body: 'Используйте Codex или Claude Code для кода. Anda Bot держит долговечный слой помощника рядом.',
			install: 'Установить',
			docs: 'Документы',
			github: 'GitHub'
		}
	},
	ar: {
		meta: {
			title: 'Anda Bot - مساعد AI محلي يبدأ من الذاكرة',
			description:
				'استخدم Anda Bot كمساعد محلي يبدأ من الذاكرة، مع ذاكرة رسومية وسياق وتفضيلات وأدوات ومهام طويلة تحت سيطرتك.',
			ogTitle: 'Anda Bot - مساعد AI محلي يبدأ من الذاكرة',
			ogDescription:
				'ثبّت launcher، وصل إضافة المتصفح، واحتفظ بالذاكرة الرسومية طويلة المدى على جهازك.'
		},
		nav: {
			install: 'تثبيت التطبيق',
			why: 'لماذا',
			browser: 'المتصفح',
			launcher: 'Launcher',
			memory: 'الذاكرة',
			docs: 'الوثائق'
		},
		language: { label: 'اللغة' },
		hero: {
			badge: 'مساعد محلي يبدأ من الذاكرة',
			title: 'يمكن تغيير النموذج. الذاكرة لا يجب أن تضيع',
			body: 'يحفظ Anda Bot الذاكرة الرسومية محلياً كي يستمر مساعدك عبر المنصات والنماذج والجلسات.',
			primary: 'تثبيت التطبيق',
			secondary: 'إضافة الامتداد'
		},
		proof: [
			{ value: 'memory-first', label: 'مبني حول ذاكرة محلية، لا حول حساب نموذج واحد' },
			{ value: 'portable', label: 'غيّر النماذج دون إعادة بناء السياق والتفضيلات' },
			{
				value: 'surfaces',
				label: 'المتصفح و launcher والطرفية و skills و cron وقنوات IM تشارك Brain واحداً'
			}
		],
		why: {
			badge: 'لماذا Anda Bot',
			title: 'وكلاء البرمجة للكود. Anda Bot للاستمرارية',
			body: 'Claude Code و Codex ممتازان داخل المستودع. Anda Bot هو طبقة المساعد الطويلة التي تتذكرك.',
			routes: [
				{
					name: 'Claude Code و Codex',
					role: 'جلسات برمجة مركزة',
					fit: 'الأفضل عندما يكون المستودع هو السياق ولا تحتاج الذاكرة الشخصية بعد انتهاء المهمة.'
				},
				{
					name: 'OpenClaw ومنصات شبيهة Hermes',
					role: 'أدوات و plugins واسعة',
					fit: 'الأفضل عندما تكون الأولوية للنظام البيئي و skills الجاهزة وكثرة القدرات المضمنة.'
				},
				{
					name: 'Anda Bot',
					role: 'قاعدة مساعد شخصي',
					fit: 'الأفضل عندما يجب أن تبقى التفضيلات والعلاقات والبحوث والروتين والهوية بعد تغيير النموذج.',
					primary: true
				}
			]
		},
		install: {
			badge: 'ابدأ',
			title: 'ثبّت التطبيق الذي يملك الذاكرة',
			body: 'ابدأ من launcher، وصل المتصفح، وأبق daemon و Brain يعملان محلياً.',
			detected: 'تم اكتشاف {os}',
			chooseOs: 'اختر النظام',
			tabAria: 'مسار التثبيت حسب نظام التشغيل',
			copy: 'نسخ',
			copied: 'تم النسخ',
			copyFailed: 'فشل النسخ',
			copyAria: 'نسخ أمر التثبيت',
			commandAria: 'نسخ أمر التثبيت',
			options: {
				macos: {
					label: 'macOS',
					title: 'Menu-bar launcher',
					body: 'يثبت السكربت Anda Bot.app، ويسجل launcher عند تسجيل الدخول، ويبدأ daemon بعد الإعداد.',
					primaryLabel: 'نسخ المثبت',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'سكريبت macOS',
					note: 'يمكن للـ launcher فحص التحديثات، وإعادة تشغيل daemon، وفتح السجلات، وإنشاء رموز ربط المتصفح.',
					steps: ['تثبيت التطبيق', 'إعداد النموذج', 'وصل المتصفح']
				},
				windows: {
					label: 'Windows',
					title: 'مثبت رسومي',
					body: 'نزّل تطبيق setup الأحدث. يثبت launcher والاختصارات و curated skills ومعالج الإعداد.',
					primaryLabel: 'تنزيل المثبت',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: 'يبدأ tray launcher عند تسجيل الدخول، ويدير daemon، ويخبرك عندما يصبح التحديث جاهزاً.',
					steps: ['تشغيل setup', 'استخدام المعالج', 'وصل المتصفح']
				},
				linux: {
					label: 'Linux',
					title: 'تثبيت daemon محلي',
					body: 'يحافظ Linux على runtime بنمط CLI-first مع autostart للـ daemon. اللوحة الجانبية تتصل بالـ gateway المحلي نفسه.',
					primaryLabel: 'نسخ المثبت',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'سكريبت Linux',
					note: 'هذا المسار مناسب لمحطات العمل والخوادم ومن يفضل إدارة runtime مباشرة.',
					steps: ['تثبيت runtime', 'إعداد provider', 'وصل المتصفح']
				}
			}
		},
		browser: {
			badge: 'لوحة المتصفح الجانبية',
			title: 'يعطي المتصفح Anda جسداً على الويب',
			body: 'اسأل عن الصفحة النشطة، اجمع الأدلة، ودع daemon المحلي يعمل عبر أدوات المتصفح بعد موافقتك.',
			store: 'إضافة الامتداد',
			docs: 'وصل المتصفح',
			features: [
				{
					title: 'إدخال سياق الصفحة في الذاكرة',
					detail:
						'أرسل العنوان والرابط والتحديد والنص واللقطات والبيانات المنظمة وسياق الوصول إلى الوكيل المحلي.'
				},
				{
					title: 'العمل بإذن',
					detail: 'افتح تبويبات، بدّل صفحات، انقر، اكتب، مرر، نزّل، اطبع PDF، وافحص العناصر.'
				},
				{
					title: 'استخدام Brain نفسه',
					detail: 'عمل المتصفح يتصل بنفس daemon والملفات والأدوات و skills والقنوات وذاكرة Brain.'
				}
			]
		},
		launcher: {
			badge: 'Desktop launcher',
			title: 'تطبيق مقيم ل Brain المحلي',
			body: 'يبقى setup والحالة والربط والسجلات وإعادة التشغيل والتحديثات قرب النظام لاستخدام Anda يومياً.',
			features: [
				{
					title: 'الإعداد الأول',
					detail: 'اضبط provider و API key والنموذج و home دون البحث في ملفات الإعداد.'
				},
				{
					title: 'التحكم بالـ daemon',
					detail: 'افتح Anda، افحص الحالة، أعد تشغيل daemon، عدل النموذج، وافتح السجلات من القائمة.'
				},
				{
					title: 'ربط المتصفح',
					detail: 'أنشئ Gateway URL و Bearer token من launcher ثم الصقهما في اللوحة الجانبية.'
				},
				{
					title: 'تنبيهات التحديث',
					detail: 'افحص وحمّل وثبّت التحديثات مع مطالبة بإعادة التشغيل عند الجاهزية.'
				}
			]
		},
		memory: {
			badge: 'Anda Brain',
			title: 'Brain هو جوهر المنتج',
			body: 'النماذج واجهات استدلال. الأصل الدائم هو الرسم المحلي للمشاريع والتفضيلات والعلاقات والقرارات.',
			features: [
				{
					title: 'رسم معرفة محلي',
					detail:
						'يشكل Brain شبكة Cognitive Nexus من أشخاص ومشاريع وتفضيلات وقرارات وأحداث وحقائق متغيرة.'
				},
				{
					title: 'هوية مستمرة',
					detail: 'يحمل Anda سياقك وأسلوب عملك ومسؤولياتك المتكررة وعلاقاتك الموثوقة بين الجلسات.'
				},
				{
					title: 'ذاكرة عبر النماذج',
					detail: 'يمكن استبدال النموذج. تبقى الذاكرة محلية وقابلة للفحص ومستقلة عن مزود واحد.'
				},
				{
					title: 'سياق واع بالأدوات',
					detail:
						'الملفات و shell والمهام المجدولة والمستندات والمتصفح و subagents تغذي السياق نفسه.'
				}
			]
		},
		work: {
			badge: 'الاختيار',
			title: 'ليس أكبر صندوق أدوات. بل المساعد الأطول عمراً',
			body: 'يركز Anda Bot أولاً على جودة الذاكرة واستكشاف العالم وتجربة الاستخدام اليومية، لا على حزم كل ميزة.',
			surfaces: [
				{
					label: 'آلية الذاكرة',
					detail: 'اجعل تذكر التفضيلات والمشاريع والعلاقات والقرارات موثوقاً قبل زيادة الضوابط.'
				},
				{
					label: 'استكشاف العالم',
					detail: 'استخدم المتصفح والمستندات والملفات و shell والمهام المجدولة لجمع الأدلة.'
				},
				{
					label: 'تجربة يومية',
					detail: 'انتقل من العمل عبر الطرفية فقط إلى launcher ولوحة جانبية صالحة للاستخدام الطويل.'
				},
				{
					label: 'حواف مفتوحة',
					detail: 'حافظ على skills والأدوات و subagents ومساعدي البرمجة دون حبس الذاكرة.'
				}
			]
		},
		final: {
			title: 'ابن على ذاكرة تملكها.',
			body: 'استخدم Codex أو Claude Code للبرمجة المركزة. دع Anda Bot يحفظ طبقة المساعد الدائمة.',
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
