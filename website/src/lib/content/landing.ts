export type Locale = 'ar' | 'zh' | 'en' | 'fr' | 'ru' | 'es';
export type TextDirection = 'ltr' | 'rtl';
export type OsKey = 'macos' | 'windows' | 'linux';
type FeatureCopy = { title: string; detail: string };
type SectionCopy = { badge: string; title: string; body: string; features: FeatureCopy[] };
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
type Five = [string, string, string, string, string];
/** Strings for the live product scenes stitched along the landing thread. */
type DemoCopy = {
	composer: {
		status: string;
		placeholder: string;
		turns: [ComposerTurn, ComposerTurn];
	};
	memory: {
		kinds: Five;
		items: Five;
		revised: string;
		source: string;
		updated: string;
		linked: string;
	};
	action: {
		browser: string;
		summary: string;
		goal: string;
		agents: [string, string, string];
		routines: [RoutineCopy, RoutineCopy];
		next: string;
	};
	control: {
		machine: string;
		items: [string, string, string];
		model: string;
		stays: string;
	};
};
type ComposerTurn = { user: string; reply: string; note: string };
type RoutineCopy = { when: string; what: string };
export type LandingCopy = {
	meta: { title: string; description: string };
	nav: {
		memory: string;
		action: string;
		control: string;
		surfaces: string;
		install: string;
		docs: string;
	};
	language: { label: string };
	hero: {
		badge: string;
		title: [string, string, string];
		body: string;
		primary: string;
		secondary: string;
		facts: [string, string, string];
	};
	pillars: [FeatureCopy, FeatureCopy, FeatureCopy];
	memory: SectionCopy;
	memoryLink: string;
	memoryPreview: string;
	previewLabel: string;
	action: SectionCopy;
	control: SectionCopy;
	dataNote: string;
	privacyLink: string;
	sourceLink: string;
	surfaces: {
		badge: string;
		title: string;
		body: string;
		items: { title: string; detail: string; linkLabel: string; status?: string }[];
	};
	install: {
		badge: string;
		title: string;
		body: string;
		requirements: string;
		detected: string;
		osLabel: string;
		copy: string;
		copied: string;
		copyFailed: string;
		copyAria: string;
		options: Record<OsKey, InstallOptionCopy>;
	};
	faqTitle: string;
	faq: FeatureCopy[];
	final: { title: string; body: string; install: string; docs: string };
	demo: DemoCopy;
};
export const fallbackLocale: Locale = 'en';
export const localeOrder: Locale[] = ['en', 'zh', 'es', 'fr', 'ru', 'ar'];
export const localeMeta: Record<
	Locale,
	{ label: string; nativeName: string; htmlLang: string; dir: TextDirection }
> = {
	en: { label: 'English', nativeName: 'English', htmlLang: 'en', dir: 'ltr' },
	zh: { label: 'Chinese', nativeName: '中文', htmlLang: 'zh-CN', dir: 'ltr' },
	es: { label: 'Spanish', nativeName: 'Español', htmlLang: 'es', dir: 'ltr' },
	fr: { label: 'French', nativeName: 'Français', htmlLang: 'fr', dir: 'ltr' },
	ru: { label: 'Russian', nativeName: 'Русский', htmlLang: 'ru', dir: 'ltr' },
	ar: { label: 'Arabic', nativeName: 'العربية', htmlLang: 'ar', dir: 'rtl' }
};

const releaseDownload = 'https://github.com/ldclabs/anda-bot/releases/latest/download';
const macInstallerFileName = 'Anda-mac-arm64.dmg';
const macInstallerUrl = `${releaseDownload}/${macInstallerFileName}`;
const windowsInstallerFileName = 'Anda-win-x64.exe';
const windowsInstallerUrl = `${releaseDownload}/${windowsInstallerFileName}`;
const desktopCaskCommand = 'brew install --cask ldclabs/tap/anda-desktop';

export const landingCopy: Record<Locale, LandingCopy> = {
	en: {
		meta: {
			title: 'Anda Bot — Memory. Action. Your control.',
			description:
				'A local AI assistant that remembers useful context and works with your browser, files, and tools. Choose your models and keep your memory on your machine.'
		},
		nav: {
			memory: 'Memory',
			action: 'Action',
			control: 'Your control',
			surfaces: 'Ways to use',
			install: 'Get started',
			docs: 'Docs'
		},
		language: {
			label: 'Language'
		},
		hero: {
			badge: 'Your local AI assistant',
			title: ['Memory.', 'Action.', 'Your control.'],
			body: 'Anda Bot brings useful context from past conversations into the work ahead. Give it a task, connect your tools, and build on what matters—with models you choose and memory on your machine.',
			primary: 'Get started',
			secondary: 'Explore Anda',
			facts: ['Open source', 'Memory stored locally', 'Your choice of models']
		},
		pillars: [
			{
				title: 'Remember what matters',
				detail: 'Keep useful preferences, context, and decisions available for later work.'
			},
			{
				title: 'Put ideas into action',
				detail: 'Work with the web, local files, tools, and scheduled tasks.'
			},
			{
				title: 'Keep the choice yours',
				detail: 'Choose your models and manage your own local data.'
			}
		],
		memory: {
			badge: '01 / Memory',
			title: 'Let useful context accumulate.',
			body: 'Your preferences, projects, and important decisions can become lasting context. Anda Brain organizes useful information from conversations and recalls it when relevant.',
			features: [
				{
					title: 'Your working preferences',
					detail: 'Carry the details that shape how you like things done into future conversations.'
				},
				{
					title: 'Connected context',
					detail: 'Connect people, projects, events, and decisions in a local knowledge graph.'
				},
				{
					title: 'Room for things to change',
					detail:
						'Tell Anda when information changes, so new context can become part of its memory.'
				}
			]
		},
		memoryLink: 'Explore how memory works',
		memoryPreview:
			'In the next release: inspect memory sources and review corrections or removals for supported records.',
		previewLabel: 'Next release',
		action: {
			badge: '02 / Action',
			title: 'Bring your assistant into the work.',
			body: 'Ask questions, explore information, or hand over a longer task. Anda can use the tools you connect and keep working toward a defined goal.',
			features: [
				{
					title: 'Browse and explore',
					detail:
						'Read web pages, gather information, and interact with sites through browser tools.'
				},
				{
					title: 'Work with files and tools',
					detail:
						'Read and write local files, run commands, and extend workflows with skills and MCP.'
				},
				{
					title: 'Keep longer tasks moving',
					detail: 'Maintain a goal, coordinate subagents, and carry work forward as context grows.'
				},
				{
					title: 'Make room for routines',
					detail: 'Schedule one-off or recurring tasks while your Anda runtime is running.'
				}
			]
		},
		control: {
			badge: '03 / Your control',
			title: 'Your models. Your memory. Your choices.',
			body: 'Keep a lasting assistant without tying its memory to one model account.',
			features: [
				{
					title: 'Data on your machine',
					detail: 'Keep configuration, conversations, and memory in your local Anda home directory.'
				},
				{
					title: 'Models you choose',
					detail: 'Connect supported providers and change models while retaining your local memory.'
				},
				{
					title: 'Open by design',
					detail:
						'Inspect the source, configure your tools, and adapt the assistant to your own workflow.'
				}
			]
		},
		dataNote:
			'Local storage does not mean offline processing. Relevant content may be sent to the model providers and services you configure.',
		privacyLink: 'How data is handled',
		sourceLink: 'Explore the source',
		surfaces: {
			badge: 'Ways to use Anda',
			title: 'An assistant within reach.',
			body: 'Choose the interface that fits how you work. Each connects to the Anda runtime you configure.',
			items: [
				{
					title: 'Browser',
					detail:
						'Keep Anda beside the page you are reading. Connect the Chrome or Edge extension to your local runtime.',
					linkLabel: 'Set up the extension'
				},
				{
					title: 'Terminal',
					detail:
						'Chat, select a workspace, and use local tools from the CLI and terminal interface.',
					linkLabel: 'Use the terminal'
				},
				{
					title: 'Messaging',
					detail:
						'Connect Telegram, WeChat, Discord, or Lark/Feishu. Access follows your channel configuration.',
					linkLabel: 'Connect a channel'
				},
				{
					title: 'Desktop workbench',
					detail:
						'Chat, memory, browser, terminal, and Git in one desktop app, with a tray that keeps Anda running.',
					linkLabel: 'About Anda Desktop'
				}
			]
		},
		faqTitle: 'Before you start',
		faq: [
			{
				title: 'Do I need to set up a separate memory service?',
				detail:
					'Ordinary long-term memory uses your model configuration. You do not need to configure the advanced Brain runtime or learning features to start.'
			},
			{
				title: 'Is everything processed on my computer?',
				detail:
					'Memory and runtime state are stored locally. Model calls, speech services, and connected tools may process content elsewhere, depending on your configuration.'
			},
			{
				title: 'Can I use just the browser extension?',
				detail:
					'The extension connects to a running Anda runtime; it is not a standalone hosted assistant. Install and configure Anda first, then pair the extension.'
			},
			{
				title: 'Can it work while the app is closed?',
				detail:
					'Tasks require a running Anda runtime and access to the configured services. Closing a client and stopping the runtime are different actions; work cannot run while the computer is shut down.'
			}
		],
		final: {
			title: 'Make room for an assistant of your own.',
			body: 'Start a conversation, connect your tools, and let useful context accumulate through the work you do together.',
			install: 'Get started',
			docs: 'Read the docs'
		},
		demo: {
			composer: {
				status: 'Local runtime',
				placeholder: 'Message Anda…',
				turns: [
					{
						user: 'Keep replies short. Mia leads the Atlas redesign, and we ship Friday.',
						reply: 'Got it. I’ll remember that.',
						note: '3 memories saved'
					},
					{
						user: 'Draft a launch checklist from docs/launch.md.',
						reply: 'Done. checklist.md is ready.',
						note: 'read_file · write_file'
					}
				]
			},
			memory: {
				kinds: ['Preference', 'Project', 'Person', 'Decision', 'Event'],
				items: [
					'Short, direct replies',
					'Atlas redesign',
					'Mia · design lead',
					'Ship on Friday',
					'Launch review · Oct 12'
				],
				revised: 'Ship on Monday',
				source: 'From chat',
				updated: 'Updated',
				linked: 'Linked'
			},
			action: {
				browser: '3 sources read',
				summary: 'Summary',
				goal: 'Prepare the Atlas launch',
				agents: ['Research', 'Draft', 'Review'],
				routines: [
					{ when: 'Mon 09:00', what: 'Weekly report' },
					{ when: 'Daily 18:00', what: 'Inbox digest' }
				],
				next: 'Next run'
			},
			control: {
				machine: 'Your machine',
				items: ['Configuration', 'Conversations', 'Memory'],
				model: 'Model provider',
				stays: 'Memory stays local'
			}
		},
		install: {
			badge: 'Get started',
			title: 'Start with something you want to do.',
			body: 'Install the local runtime, connect a model provider, and open your preferred interface. The current installers include the CLI and launcher where supported.',
			detected: 'Detected {os}',
			osLabel: 'Install path by operating system',
			copy: 'Copy',
			copied: 'Copied',
			copyFailed: 'Copy failed',
			copyAria: 'Copy install command',
			options: {
				macos: {
					label: 'macOS',
					title: 'Anda Desktop',
					body: 'Download Anda for Apple Silicon. It installs the anda command line, curated skills, and a menu-bar tray that keeps Anda running at login.',
					primaryLabel: 'Download for Mac',
					href: macInstallerUrl,
					download: macInstallerFileName,
					command: desktopCaskCommand,
					commandLabel: 'Homebrew',
					note: 'New macOS releases support Apple Silicon only; Intel Mac users can build from source. Command line only: use the install script.',
					steps: ['Install app', 'Enter model settings', 'Pair browser']
				},
				windows: {
					label: 'Windows',
					title: 'Anda Desktop',
					body: 'Download the installer. It installs Anda Desktop, the anda command line, curated skills, and a tray that keeps Anda running at login.',
					primaryLabel: 'Download installer',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: 'The tray shows service status, installs updates, restarts the service, opens logs, and copies browser pairing tokens.',
					steps: ['Run setup', 'Enter model settings', 'Pair browser']
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
			},
			requirements:
				'You need a supported model provider and usually an API key. Model and connected-service usage may incur separate charges.'
		}
	},
	zh: {
		meta: {
			title: 'Anda Bot — 有记忆，能做事，由你掌握',
			description:
				'运行在你自己电脑上的 AI 助手。积累重要背景，使用浏览器、本地文件和工具协助工作。模型由你选择，长期记忆保存在本机。'
		},
		nav: {
			memory: '有记忆',
			action: '能做事',
			control: '由你掌握',
			surfaces: '使用入口',
			install: '开始使用',
			docs: '文档'
		},
		language: {
			label: '语言'
		},
		hero: {
			badge: '属于你的本地 AI 助手',
			title: ['有记忆，', '能做事，', '由你掌握。'],
			body: 'Anda Bot 将有用的对话背景带入之后的工作。交给它任务，连接你的工具，让重要的积累持续发挥作用。模型由你选择，长期记忆保存在自己的电脑上。',
			primary: '开始使用',
			secondary: '了解 Anda',
			facts: ['开源', '记忆保存在本机', '自由选择模型']
		},
		pillars: [
			{
				title: '记住重要的事',
				detail: '积累偏好、背景与重要决定，为之后的工作提供上下文。'
			},
			{
				title: '把想法付诸行动',
				detail: '使用网页、本地文件、工具与定时任务，协助你推进事情。'
			},
			{
				title: '选择留在自己手中',
				detail: '选择模型服务，管理自己的本地数据。'
			}
		],
		memory: {
			badge: '01 / 有记忆',
			title: '让有用的背景，\n逐渐积累。',
			body: '你的偏好、项目和重要决定，都可以成为之后工作的背景。Anda Brain 从对话中整理有用信息，在相关任务中按需召回。',
			features: [
				{
					title: '记住工作偏好',
					detail: '将你对表达方式、工作习惯和处理细节的偏好带入之后的对话。'
				},
				{
					title: '连接相关信息',
					detail: '通过本地知识图谱，关联人物、项目、事件与决定。'
				},
				{
					title: '跟上信息的变化',
					detail: '当情况发生变化，告诉 Anda，让新的背景也进入记忆。'
				}
			]
		},
		memoryLink: '了解记忆如何工作',
		memoryPreview: '下一版本：查看记忆来源，对支持的记录核对更正、停用或删除的范围。',
		previewLabel: '新版进展',
		action: {
			badge: '02 / 能做事',
			title: '让助手参与到实际工作中。',
			body: '随时提问、探索信息，或交给它一个需要持续推进的任务。Anda 可以调用你连接的工具，围绕明确的目标继续工作。',
			features: [
				{
					title: '浏览与探索',
					detail: '读取网页、收集资料，通过浏览器工具与网站交互。'
				},
				{
					title: '处理文件与调用工具',
					detail: '读写本地文件、执行命令，通过技能与 MCP 扩展工作方式。'
				},
				{
					title: '推进长期任务',
					detail: '保持目标、协调子智能体，在上下文增长时继续推进工作。'
				},
				{
					title: '安排日常事务',
					detail: '在 Anda 后台服务运行期间，执行一次性或周期性的定时任务。'
				}
			]
		},
		control: {
			badge: '03 / 由你掌握',
			title: '模型可以换，积累留下来。',
			body: '让助手长期为你服务，让记忆独立于单一模型账号。',
			features: [
				{
					title: '数据保存在本机',
					detail: '配置、对话和记忆保存在本地 Anda 数据目录中。'
				},
				{
					title: '模型由你选择',
					detail: '连接支持的模型服务商，更换模型时保留本地记忆。'
				},
				{
					title: '保持开放',
					detail: '查看源代码，配置工具，根据自己的工作方式调整助手。'
				}
			]
		},
		dataNote: '本地保存不等于离线处理。相关内容可能发送给你配置的模型服务商与外部服务。',
		privacyLink: '了解数据如何处理',
		sourceLink: '查看源代码',
		surfaces: {
			badge: '使用入口',
			title: '在顺手的地方，找到 Anda。',
			body: '选择适合自己的使用方式，连接你配置的 Anda 运行环境。',
			items: [
				{
					title: '浏览器',
					detail: '在浏览网页时打开 Anda。将 Chrome 或 Edge 扩展连接到本地运行环境。',
					linkLabel: '配置浏览器扩展'
				},
				{
					title: '终端',
					detail: '通过命令行与终端界面聊天、选择工作目录，使用本地工具。',
					linkLabel: '在终端中使用'
				},
				{
					title: '消息频道',
					detail: '接入 Telegram、微信、Discord 或飞书/Lark，访问范围遵循你的频道配置。',
					linkLabel: '连接消息频道'
				},
				{
					title: '桌面工作台',
					detail: '在一个桌面应用中使用聊天、记忆、浏览器、终端与 Git，托盘让 Anda 持续运行。',
					linkLabel: '了解 Anda 桌面端'
				}
			]
		},
		faqTitle: '开始之前',
		faq: [
			{
				title: '需要单独配置记忆服务吗？',
				detail:
					'普通长期记忆沿用你的模型配置。开始使用时，无需额外配置 Brain 高级运行时或学习功能。'
			},
			{
				title: '所有内容都在本机处理吗？',
				detail:
					'记忆和运行状态保存在本机。模型调用、语音服务和连接的工具可能在其他地方处理内容，具体取决于你的配置。'
			},
			{
				title: '只安装浏览器扩展可以使用吗？',
				detail:
					'扩展需要连接正在运行的 Anda 服务，并非独立的云端助手。先安装和配置 Anda，再完成扩展配对。'
			},
			{
				title: '关闭应用后还能继续工作吗？',
				detail:
					'任务需要 Anda 后台服务保持运行，并能访问配置的服务。关闭客户端与停止后台服务是不同的操作；电脑关机时无法继续执行。'
			}
		],
		final: {
			title: '让自己的 AI 助手，从这里开始。',
			body: '开始对话，连接工具，让有用的背景在一次次协作中积累。',
			install: '开始使用',
			docs: '阅读文档'
		},
		demo: {
			composer: {
				status: '本地运行',
				placeholder: '给 Anda 发消息…',
				turns: [
					{
						user: '回复简短一点。Mia 负责 Atlas 改版，我们周五发布。',
						reply: '好的，我记住了。',
						note: '已保存 3 条记忆'
					},
					{
						user: '根据 docs/launch.md 起草一份发布清单。',
						reply: '完成，checklist.md 已就绪。',
						note: 'read_file · write_file'
					}
				]
			},
			memory: {
				kinds: ['偏好', '项目', '人物', '决定', '事件'],
				items: [
					'回复简洁直接',
					'Atlas 改版',
					'Mia · 设计负责人',
					'周五发布',
					'10 月 12 日上线评审'
				],
				revised: '改为周一发布',
				source: '来自对话',
				updated: '已更新',
				linked: '已关联'
			},
			action: {
				browser: '已阅读 3 个来源',
				summary: '摘要',
				goal: '准备 Atlas 发布',
				agents: ['调研', '起草', '审阅'],
				routines: [
					{ when: '周一 09:00', what: '每周报告' },
					{ when: '每天 18:00', what: '收件箱摘要' }
				],
				next: '下次运行'
			},
			control: {
				machine: '你的电脑',
				items: ['配置', '对话', '记忆'],
				model: '模型服务商',
				stays: '记忆保存在本机'
			}
		},
		install: {
			badge: '快速开始',
			title: '从你想做的事开始。',
			body: '安装本地运行环境，连接模型服务，打开顺手的使用入口。当前安装程序提供命令行工具，并在支持的平台提供启动器。',
			detected: '检测到 {os}',
			osLabel: '各操作系统的安装路径',
			copy: '复制',
			copied: '已复制',
			copyFailed: '复制失败',
			copyAria: '复制安装命令',
			options: {
				macos: {
					label: 'macOS',
					title: 'Anda 桌面端',
					body: '下载适用于 Apple Silicon 的 Anda。它会安装 anda 命令行、精选技能，以及登录时让 Anda 保持运行的菜单栏托盘。',
					primaryLabel: '下载 Mac 版',
					href: macInstallerUrl,
					download: macInstallerFileName,
					command: desktopCaskCommand,
					commandLabel: 'Homebrew',
					note: '新的 macOS 发布包仅支持 Apple Silicon；Intel Mac 用户可从源码构建。只需要命令行时，可使用安装脚本。',
					steps: ['安装应用', '配置模型', '连接浏览器']
				},
				windows: {
					label: 'Windows',
					title: 'Anda 桌面端',
					body: '下载安装程序。它会安装 Anda 桌面端、anda 命令行、精选技能，以及登录时让 Anda 保持运行的托盘。',
					primaryLabel: '下载安装程序',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: '托盘可查看服务状态、安装更新、重启服务、打开日志，并复制浏览器配对令牌。',
					steps: ['运行安装', '配置模型', '连接浏览器']
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
			},
			requirements:
				'需要支持的模型服务，通常需要自行提供 API Key。模型调用与外部服务可能产生单独费用。'
		}
	},
	es: {
		meta: {
			title: 'Anda Bot — Memoria. Acción. Tú decides.',
			description:
				'Un asistente de IA local que recuerda el contexto útil y trabaja con tu navegador, archivos y herramientas. Elige tus modelos y conserva la memoria en tu equipo.'
		},
		nav: {
			memory: 'Memoria',
			action: 'Acción',
			control: 'Tú decides',
			surfaces: 'Cómo usarlo',
			install: 'Empezar',
			docs: 'Documentación'
		},
		language: {
			label: 'Idioma'
		},
		hero: {
			badge: 'Tu asistente de IA local',
			title: ['Memoria.', 'Acción.', 'Tú decides.'],
			body: 'Anda Bot lleva el contexto útil de tus conversaciones al trabajo que viene. Asígnale tareas, conecta tus herramientas y aprovecha lo que has acumulado, con los modelos que elijas y la memoria en tu equipo.',
			primary: 'Empezar',
			secondary: 'Conocer Anda',
			facts: ['Código abierto', 'Memoria local', 'Elige tus modelos']
		},
		pillars: [
			{
				title: 'Recuerda lo importante',
				detail: 'Conserva preferencias, contexto y decisiones para trabajos futuros.'
			},
			{
				title: 'Pasa a la acción',
				detail: 'Trabaja con la web, archivos locales, herramientas y tareas programadas.'
			},
			{
				title: 'Mantén el control',
				detail: 'Elige tus modelos y administra tus datos locales.'
			}
		],
		memory: {
			badge: '01 / Memoria',
			title: 'Deja que el contexto útil se acumule.',
			body: 'Tus preferencias, proyectos y decisiones importantes pueden servir de contexto duradero. Anda Brain organiza la información útil de las conversaciones y la recupera cuando es relevante.',
			features: [
				{
					title: 'Tus preferencias de trabajo',
					detail: 'Lleva los detalles sobre cómo te gusta trabajar a futuras conversaciones.'
				},
				{
					title: 'Contexto conectado',
					detail:
						'Relaciona personas, proyectos, eventos y decisiones en un grafo de conocimiento local.'
				},
				{
					title: 'Información que evoluciona',
					detail: 'Cuéntale a Anda qué ha cambiado para incorporar el nuevo contexto a su memoria.'
				}
			]
		},
		memoryLink: 'Cómo funciona la memoria',
		memoryPreview:
			'En la próxima versión: consulta las fuentes y revisa correcciones o eliminaciones de los registros compatibles.',
		previewLabel: 'Próxima versión',
		action: {
			badge: '02 / Acción',
			title: 'Un asistente que participa en tu trabajo.',
			body: 'Haz preguntas, explora información o encarga una tarea más larga. Anda puede usar las herramientas que conectes y seguir trabajando hacia un objetivo definido.',
			features: [
				{
					title: 'Navega y explora',
					detail:
						'Lee páginas, reúne información e interactúa con sitios mediante herramientas del navegador.'
				},
				{
					title: 'Archivos y herramientas',
					detail:
						'Lee y escribe archivos locales, ejecuta comandos y amplía tus flujos con habilidades y MCP.'
				},
				{
					title: 'Avanza en tareas largas',
					detail:
						'Mantén un objetivo, coordina subagentes y continúa a medida que crece el contexto.'
				},
				{
					title: 'Organiza tus rutinas',
					detail:
						'Programa tareas únicas o recurrentes mientras el servicio de Anda esté en marcha.'
				}
			]
		},
		control: {
			badge: '03 / Tú decides',
			title: 'Cambia de modelo. Conserva tu memoria.',
			body: 'Mantén un asistente duradero sin vincular su memoria a una sola cuenta de modelos.',
			features: [
				{
					title: 'Datos en tu equipo',
					detail:
						'Guarda la configuración, las conversaciones y la memoria en tu directorio local de Anda.'
				},
				{
					title: 'Modelos a tu elección',
					detail: 'Conecta proveedores compatibles y cambia de modelo conservando tu memoria local.'
				},
				{
					title: 'Abierto por diseño',
					detail:
						'Revisa el código, configura herramientas y adapta el asistente a tu forma de trabajar.'
				}
			]
		},
		dataNote:
			'El almacenamiento local no implica procesamiento sin conexión. El contenido relevante puede enviarse a los proveedores y servicios que configures.',
		privacyLink: 'Cómo se tratan los datos',
		sourceLink: 'Ver el código',
		surfaces: {
			badge: 'Cómo usar Anda',
			title: 'Un asistente a tu alcance.',
			body: 'Elige la interfaz que mejor se adapte a ti. Todas se conectan al entorno de Anda que configures.',
			items: [
				{
					title: 'Navegador',
					detail:
						'Ten a Anda junto a la página que lees. Conecta la extensión de Chrome o Edge a tu servicio local.',
					linkLabel: 'Configurar la extensión'
				},
				{
					title: 'Terminal',
					detail:
						'Conversa, selecciona un directorio de trabajo y usa herramientas locales desde la línea de comandos.',
					linkLabel: 'Usar la terminal'
				},
				{
					title: 'Mensajería',
					detail:
						'Conecta Telegram, WeChat, Discord o Lark/Feishu. El acceso sigue la configuración de tus canales.',
					linkLabel: 'Conectar un canal'
				},
				{
					title: 'Aplicación de escritorio',
					detail:
						'Chat, memoria, navegador, terminal y Git en una app de escritorio, con una bandeja que mantiene Anda en marcha.',
					linkLabel: 'Acerca de Anda Desktop'
				}
			]
		},
		faqTitle: 'Antes de empezar',
		faq: [
			{
				title: '¿Debo configurar un servicio de memoria aparte?',
				detail:
					'La memoria habitual utiliza tu configuración de modelos. No necesitas configurar las funciones avanzadas de Brain ni de aprendizaje para empezar.'
			},
			{
				title: '¿Se procesa todo en mi equipo?',
				detail:
					'La memoria y el estado se guardan localmente. Las llamadas a modelos, los servicios de voz y las herramientas pueden procesar contenido fuera del equipo según tu configuración.'
			},
			{
				title: '¿Basta con instalar la extensión?',
				detail:
					'La extensión necesita conectarse a un servicio de Anda en ejecución. No es un asistente alojado independiente. Instala y configura Anda antes de vincularla.'
			},
			{
				title: '¿Puede trabajar con la aplicación cerrada?',
				detail:
					'Las tareas requieren que el servicio de Anda siga activo y pueda acceder a los servicios configurados. Cerrar un cliente no equivale a detener el servicio. No puede ejecutar tareas con el equipo apagado.'
			}
		],
		final: {
			title: 'Haz sitio para tu propio asistente.',
			body: 'Empieza una conversación, conecta tus herramientas y acumula contexto útil a través del trabajo compartido.',
			install: 'Empezar',
			docs: 'Leer la documentación'
		},
		demo: {
			composer: {
				status: 'Servicio local',
				placeholder: 'Escribe a Anda…',
				turns: [
					{
						user: 'Respuestas breves. Mia lidera el rediseño de Atlas y lanzamos el viernes.',
						reply: 'Entendido. Lo tendré en cuenta.',
						note: '3 recuerdos guardados'
					},
					{
						user: 'Prepara una lista de lanzamiento a partir de docs/launch.md.',
						reply: 'Hecho. checklist.md está lista.',
						note: 'read_file · write_file'
					}
				]
			},
			memory: {
				kinds: ['Preferencia', 'Proyecto', 'Persona', 'Decisión', 'Evento'],
				items: [
					'Respuestas breves y directas',
					'Rediseño de Atlas',
					'Mia · lidera el diseño',
					'Lanzar el viernes',
					'Revisión de lanzamiento · 12 oct'
				],
				revised: 'Lanzar el lunes',
				source: 'De la conversación',
				updated: 'Actualizado',
				linked: 'Relacionado'
			},
			action: {
				browser: '3 fuentes leídas',
				summary: 'Resumen',
				goal: 'Preparar el lanzamiento de Atlas',
				agents: ['Investigar', 'Redactar', 'Revisar'],
				routines: [
					{ when: 'Lun 09:00', what: 'Informe semanal' },
					{ when: 'Diario 18:00', what: 'Resumen del correo' }
				],
				next: 'Próxima ejecución'
			},
			control: {
				machine: 'Tu equipo',
				items: ['Configuración', 'Conversaciones', 'Memoria'],
				model: 'Proveedor de modelos',
				stays: 'La memoria se guarda en local'
			}
		},
		install: {
			badge: 'Comenzar',
			title: 'Empieza con algo que quieras hacer.',
			body: 'Instala Anda, conecta un proveedor de modelos y abre tu interfaz preferida. Los instaladores actuales incluyen la CLI y el lanzador en las plataformas compatibles.',
			detected: 'Detectado: {os}',
			osLabel: 'Ruta de instalación por sistema operativo',
			copy: 'Copiar',
			copied: 'Copiado',
			copyFailed: 'No se pudo copiar',
			copyAria: 'Copiar comando de instalación',
			options: {
				macos: {
					label: 'macOS',
					title: 'Anda Desktop',
					body: 'Descargue Anda para Apple Silicon. Instala la línea de comandos anda, habilidades seleccionadas y una bandeja en la barra de menú que mantiene Anda en marcha al iniciar sesión.',
					primaryLabel: 'Descargar para Mac',
					href: macInstallerUrl,
					download: macInstallerFileName,
					command: desktopCaskCommand,
					commandLabel: 'Homebrew',
					note: 'Las nuevas versiones para macOS solo admiten Apple Silicon; en Mac con Intel puede compilar desde el código fuente. Solo línea de comandos: use el script de instalación.',
					steps: ['Instalar app', 'Configurar modelo', 'Emparejar navegador']
				},
				windows: {
					label: 'Windows',
					title: 'Anda Desktop',
					body: 'Descargue el instalador. Instala Anda Desktop, la línea de comandos anda, habilidades seleccionadas y una bandeja que mantiene Anda en marcha al iniciar sesión.',
					primaryLabel: 'Descargar instalador',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: 'La bandeja muestra el estado del servicio, instala actualizaciones, reinicia el servicio, abre registros y copia tokens de emparejamiento del navegador.',
					steps: ['Ejecutar setup', 'Configurar modelo', 'Emparejar navegador']
				},
				linux: {
					label: 'Linux',
					title: 'Instalación del daemon local',
					body: 'Linux conserva el runtime CLI-first con inicio automático del daemon. El panel lateral del navegador se conecta a la misma puerta de enlace local.',
					primaryLabel: 'Copiar instalador',
					command:
						'curl -fsSL https://raw.githubusercontent.com/ldclabs/anda-bot/main/scripts/install.sh | sh',
					commandLabel: 'Script de instalación de Linux',
					note: 'Usa esta opción en estaciones de trabajo, servidores o si prefieres gestionar el runtime directamente.',
					steps: ['Instalar runtime', 'Configurar proveedor', 'Emparejar navegador']
				}
			},
			requirements:
				'Necesitas un proveedor compatible y, normalmente, una clave API. Los modelos y servicios conectados pueden tener costes adicionales.'
		}
	},
	fr: {
		meta: {
			title: 'Anda Bot — Mémoire. Action. À vous de choisir.',
			description:
				'Un assistant IA local qui retient le contexte utile et travaille avec votre navigateur, vos fichiers et vos outils. Choisissez vos modèles et gardez la mémoire sur votre machine.'
		},
		nav: {
			memory: 'Mémoire',
			action: 'Action',
			control: 'Vos choix',
			surfaces: 'Accès',
			install: 'Démarrer',
			docs: 'Documentation'
		},
		language: {
			label: 'Langue'
		},
		hero: {
			badge: 'Votre assistant IA local',
			title: ['Mémoire.', 'Action.', 'Vos choix.'],
			body: 'Anda Bot apporte le contexte utile de vos conversations à vos prochains travaux. Confiez-lui des tâches, connectez vos outils et profitez de vos acquis, avec les modèles de votre choix et une mémoire sur votre machine.',
			primary: 'Démarrer',
			secondary: 'Découvrir Anda',
			facts: ['Code ouvert', 'Mémoire locale', 'Modèles au choix']
		},
		pillars: [
			{
				title: 'Retenir ce qui compte',
				detail: 'Conservez préférences, contexte et décisions pour vos prochains travaux.'
			},
			{
				title: 'Passer à l’action',
				detail: 'Travaillez avec le Web, les fichiers locaux, les outils et les tâches planifiées.'
			},
			{
				title: 'Garder la main',
				detail: 'Choisissez vos modèles et gérez vos données locales.'
			}
		],
		memory: {
			badge: '01 / Mémoire',
			title: 'Laissez le contexte utile s’enrichir.',
			body: 'Vos préférences, projets et décisions importantes peuvent former un contexte durable. Anda Brain organise les informations utiles des conversations et les retrouve quand elles sont pertinentes.',
			features: [
				{
					title: 'Vos préférences de travail',
					detail:
						'Retrouvez dans vos prochaines conversations les détails de votre façon de travailler.'
				},
				{
					title: 'Un contexte relié',
					detail:
						'Reliez personnes, projets, événements et décisions dans un graphe de connaissances local.'
				},
				{
					title: 'Des informations qui évoluent',
					detail: 'Indiquez à Anda ce qui a changé pour intégrer ce nouveau contexte à sa mémoire.'
				}
			]
		},
		memoryLink: 'Comprendre la mémoire',
		memoryPreview:
			'Prochaine version : consulter les sources et examiner les corrections ou suppressions des entrées prises en charge.',
		previewLabel: 'Prochaine version',
		action: {
			badge: '02 / Action',
			title: 'Un assistant qui participe au travail.',
			body: 'Posez une question, explorez des informations ou confiez une tâche plus longue. Anda peut utiliser vos outils et poursuivre un objectif défini.',
			features: [
				{
					title: 'Naviguer et explorer',
					detail:
						'Lisez des pages, rassemblez des informations et interagissez avec des sites grâce aux outils du navigateur.'
				},
				{
					title: 'Fichiers et outils',
					detail:
						'Lisez et écrivez des fichiers locaux, exécutez des commandes et étendez vos processus avec des compétences et MCP.'
				},
				{
					title: 'Faire avancer les tâches longues',
					detail:
						'Gardez un objectif, coordonnez des sous-agents et poursuivez le travail à mesure que le contexte grandit.'
				},
				{
					title: 'Organiser les routines',
					detail:
						'Planifiez des tâches ponctuelles ou récurrentes pendant que le service Anda fonctionne.'
				}
			]
		},
		control: {
			badge: '03 / Vos choix',
			title: 'Changez de modèle. Gardez votre mémoire.',
			body: 'Conservez un assistant durable sans lier sa mémoire à un seul compte de modèles.',
			features: [
				{
					title: 'Les données chez vous',
					detail:
						'Conservez configuration, conversations et mémoire dans votre répertoire Anda local.'
				},
				{
					title: 'Vos modèles au choix',
					detail:
						'Connectez des fournisseurs compatibles et changez de modèle en conservant la mémoire locale.'
				},
				{
					title: 'Ouvert par conception',
					detail:
						'Consultez le code, configurez les outils et adaptez l’assistant à votre façon de travailler.'
				}
			]
		},
		dataNote:
			'Stockage local ne signifie pas traitement hors ligne. Les contenus pertinents peuvent être transmis aux fournisseurs de modèles et services que vous configurez.',
		privacyLink: 'Traitement des données',
		sourceLink: 'Consulter le code',
		surfaces: {
			badge: 'Les accès à Anda',
			title: 'Un assistant à portée de main.',
			body: 'Choisissez l’interface qui vous convient. Chacune se connecte à l’environnement Anda que vous configurez.',
			items: [
				{
					title: 'Navigateur',
					detail:
						'Gardez Anda à côté de la page que vous lisez. Connectez l’extension Chrome ou Edge à votre service local.',
					linkLabel: 'Configurer l’extension'
				},
				{
					title: 'Terminal',
					detail:
						'Discutez, choisissez un répertoire de travail et utilisez vos outils locaux en ligne de commande.',
					linkLabel: 'Utiliser le terminal'
				},
				{
					title: 'Messagerie',
					detail:
						'Connectez Telegram, WeChat, Discord ou Lark/Feishu. Les accès suivent la configuration de vos canaux.',
					linkLabel: 'Connecter un canal'
				},
				{
					title: 'Application de bureau',
					detail:
						'Conversations, mémoire, navigateur, terminal et Git dans une app de bureau, avec une icône qui garde Anda actif.',
					linkLabel: 'Découvrir Anda Desktop'
				}
			]
		},
		faqTitle: 'Avant de commencer',
		faq: [
			{
				title: 'Faut-il configurer un service de mémoire séparé ?',
				detail:
					'La mémoire ordinaire utilise votre configuration de modèles. Les fonctions avancées de Brain et d’apprentissage ne sont pas nécessaires pour commencer.'
			},
			{
				title: 'Tout est-il traité sur mon ordinateur ?',
				detail:
					'La mémoire et l’état sont conservés localement. Selon votre configuration, les modèles, services vocaux et outils peuvent traiter des contenus ailleurs.'
			},
			{
				title: 'L’extension suffit-elle ?',
				detail:
					'L’extension se connecte à un service Anda actif. Ce n’est pas un assistant hébergé autonome. Installez et configurez Anda avant de l’associer.'
			},
			{
				title: 'Peut-il travailler quand l’application est fermée ?',
				detail:
					'Les tâches nécessitent un service Anda actif et l’accès aux services configurés. Fermer un client et arrêter le service sont deux actions différentes. Aucun travail ne peut s’exécuter quand l’ordinateur est éteint.'
			}
		],
		final: {
			title: 'Faites une place à votre propre assistant.',
			body: 'Lancez une conversation, connectez vos outils et enrichissez le contexte utile au fil du travail accompli ensemble.',
			install: 'Démarrer',
			docs: 'Lire la documentation'
		},
		demo: {
			composer: {
				status: 'Service local',
				placeholder: 'Écrire à Anda…',
				turns: [
					{
						user: 'Réponses courtes. Mia dirige la refonte d’Atlas, et on livre vendredi.',
						reply: 'Compris. Je m’en souviendrai.',
						note: '3 souvenirs enregistrés'
					},
					{
						user: 'Rédige une checklist de lancement à partir de docs/launch.md.',
						reply: 'Terminé. checklist.md est prête.',
						note: 'read_file · write_file'
					}
				]
			},
			memory: {
				kinds: ['Préférence', 'Projet', 'Personne', 'Décision', 'Événement'],
				items: [
					'Réponses courtes et directes',
					'Refonte d’Atlas',
					'Mia · responsable design',
					'Livrer vendredi',
					'Revue de lancement · 12 oct.'
				],
				revised: 'Livrer lundi',
				source: 'Issu de la conversation',
				updated: 'Mis à jour',
				linked: 'Relié'
			},
			action: {
				browser: '3 sources lues',
				summary: 'Résumé',
				goal: 'Préparer le lancement d’Atlas',
				agents: ['Recherche', 'Rédaction', 'Relecture'],
				routines: [
					{ when: 'Lun. 09:00', what: 'Rapport hebdo' },
					{ when: 'Chaque jour 18:00', what: 'Synthèse des messages' }
				],
				next: 'Prochaine exécution'
			},
			control: {
				machine: 'Votre machine',
				items: ['Configuration', 'Conversations', 'Mémoire'],
				model: 'Fournisseur de modèles',
				stays: 'La mémoire reste en local'
			}
		},
		install: {
			badge: 'Démarrer',
			title: 'Commencez par ce que vous voulez faire.',
			body: 'Installez Anda, connectez un fournisseur de modèles et ouvrez votre interface préférée. Les installateurs actuels incluent la CLI et le lanceur sur les plateformes compatibles.',
			detected: 'Détecté : {os}',
			osLabel: 'Chemin d’installation selon le système d’exploitation',
			copy: 'Copier',
			copied: 'Copié',
			copyFailed: 'Copie échouée',
			copyAria: 'Copier la commande d’installation',
			options: {
				macos: {
					label: 'macOS',
					title: 'Anda Desktop',
					body: 'Téléchargez Anda pour Apple Silicon. Il installe la ligne de commande anda, des compétences sélectionnées et une icône de barre de menus qui garde Anda actif dès la connexion.',
					primaryLabel: 'Télécharger pour Mac',
					href: macInstallerUrl,
					download: macInstallerFileName,
					command: desktopCaskCommand,
					commandLabel: 'Homebrew',
					note: 'Les nouvelles versions macOS sont réservées à Apple Silicon ; sur Mac Intel, compilez depuis les sources. Ligne de commande seule : utilisez le script d’installation.',
					steps: ['Installer l’app', 'Configurer le modèle', 'Appairer le navigateur']
				},
				windows: {
					label: 'Windows',
					title: 'Anda Desktop',
					body: 'Téléchargez l’installateur. Il installe Anda Desktop, la ligne de commande anda, des compétences sélectionnées et une icône qui garde Anda actif dès la connexion.',
					primaryLabel: 'Télécharger l’installateur',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: 'L’icône affiche l’état du service, installe les mises à jour, redémarre le service, ouvre les journaux et copie les jetons d’appairage du navigateur.',
					steps: ['Lancer le setup', 'Configurer le modèle', 'Appairer le navigateur']
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
			},
			requirements:
				'Il vous faut un fournisseur compatible et généralement une clé API. Les modèles et services connectés peuvent entraîner des frais distincts.'
		}
	},
	ru: {
		meta: {
			title: 'Anda Bot — Память. Действие. Ваш выбор.',
			description:
				'Локальный ИИ-помощник, который помнит полезный контекст и работает с браузером, файлами и инструментами. Выбирайте модели и храните память на своём компьютере.'
		},
		nav: {
			memory: 'Память',
			action: 'Действие',
			control: 'Ваш выбор',
			surfaces: 'Интерфейсы',
			install: 'Начать',
			docs: 'Документация'
		},
		language: {
			label: 'Язык'
		},
		hero: {
			badge: 'Ваш локальный ИИ-помощник',
			title: ['Память.', 'Действие.', 'Ваш выбор.'],
			body: 'Anda Bot использует полезный контекст прошлых разговоров в дальнейшей работе. Ставьте задачи, подключайте инструменты и опирайтесь на накопленные знания. Модели выбираете вы, а память хранится на вашем компьютере.',
			primary: 'Начать',
			secondary: 'Узнать об Anda',
			facts: ['Открытый код', 'Локальная память', 'Выбор моделей']
		},
		pillars: [
			{
				title: 'Помнить важное',
				detail: 'Сохраняйте предпочтения, контекст и решения для будущей работы.'
			},
			{
				title: 'Переходить к действию',
				detail: 'Работайте с сайтами, локальными файлами, инструментами и расписаниями.'
			},
			{
				title: 'Сохранять контроль',
				detail: 'Выбирайте модели и управляйте своими локальными данными.'
			}
		],
		memory: {
			badge: '01 / Память',
			title: 'Пусть полезный контекст накапливается.',
			body: 'Ваши предпочтения, проекты и важные решения могут стать долгосрочным контекстом. Anda Brain упорядочивает полезную информацию из разговоров и вспоминает её по мере необходимости.',
			features: [
				{
					title: 'Ваши рабочие предпочтения',
					detail: 'Учитывайте в новых разговорах детали того, как вам удобнее работать.'
				},
				{
					title: 'Связанный контекст',
					detail: 'Связывайте людей, проекты, события и решения в локальном графе знаний.'
				},
				{
					title: 'Информация меняется',
					detail: 'Рассказывайте Anda об изменениях, чтобы новый контекст тоже вошёл в память.'
				}
			]
		},
		memoryLink: 'Как работает память',
		memoryPreview:
			'В следующей версии: просмотр источников и проверка исправлений или удаления поддерживаемых записей.',
		previewLabel: 'Следующая версия',
		action: {
			badge: '02 / Действие',
			title: 'Помощник, который участвует в работе.',
			body: 'Задавайте вопросы, изучайте информацию или поручайте длительные задачи. Anda может использовать подключённые инструменты и продолжать работу над заданной целью.',
			features: [
				{
					title: 'Браузер и поиск информации',
					detail:
						'Читайте страницы, собирайте материалы и взаимодействуйте с сайтами через инструменты браузера.'
				},
				{
					title: 'Файлы и инструменты',
					detail:
						'Читайте и записывайте локальные файлы, запускайте команды и расширяйте работу с помощью навыков и MCP.'
				},
				{
					title: 'Длительные задачи',
					detail:
						'Сохраняйте цель, координируйте субагентов и продолжайте работу по мере роста контекста.'
				},
				{
					title: 'Регулярные дела',
					detail: 'Планируйте разовые или повторяющиеся задачи, пока служба Anda работает.'
				}
			]
		},
		control: {
			badge: '03 / Ваш выбор',
			title: 'Меняйте модели. Сохраняйте память.',
			body: 'Пользуйтесь постоянным помощником, не привязывая его память к одному аккаунту провайдера.',
			features: [
				{
					title: 'Данные на вашем компьютере',
					detail: 'Храните настройки, разговоры и память в локальном каталоге Anda.'
				},
				{
					title: 'Модели на ваш выбор',
					detail:
						'Подключайте поддерживаемых провайдеров и меняйте модели, сохраняя локальную память.'
				},
				{
					title: 'Открытое устройство',
					detail: 'Изучайте код, настраивайте инструменты и адаптируйте помощника к своей работе.'
				}
			]
		},
		dataNote:
			'Локальное хранение не означает обработку без сети. Нужные данные могут передаваться настроенным вами провайдерам моделей и внешним сервисам.',
		privacyLink: 'Обработка данных',
		sourceLink: 'Открыть исходный код',
		surfaces: {
			badge: 'Как использовать Anda',
			title: 'Помощник под рукой.',
			body: 'Выберите удобный интерфейс. Каждый подключается к настроенной вами среде Anda.',
			items: [
				{
					title: 'Браузер',
					detail:
						'Работайте с Anda рядом с открытой страницей. Подключите расширение Chrome или Edge к локальной службе.',
					linkLabel: 'Настроить расширение'
				},
				{
					title: 'Терминал',
					detail:
						'Общайтесь, выбирайте рабочую папку и используйте локальные инструменты через CLI и терминальный интерфейс.',
					linkLabel: 'Работа в терминале'
				},
				{
					title: 'Мессенджеры',
					detail:
						'Подключите Telegram, WeChat, Discord или Lark/Feishu. Доступ определяется настройками каналов.',
					linkLabel: 'Подключить канал'
				},
				{
					title: 'Приложение для компьютера',
					detail:
						'Чат, память, браузер, терминал и Git в одном приложении, а значок в трее поддерживает работу Anda.',
					linkLabel: 'Об Anda Desktop'
				}
			]
		},
		faqTitle: 'Перед началом',
		faq: [
			{
				title: 'Нужна ли отдельная настройка памяти?',
				detail:
					'Обычная долгосрочная память использует ваши настройки моделей. Для начала не нужны расширенная среда Brain или функции обучения.'
			},
			{
				title: 'Всё обрабатывается на моём компьютере?',
				detail:
					'Память и состояние хранятся локально. В зависимости от настроек модели, голосовые сервисы и инструменты могут обрабатывать данные вне компьютера.'
			},
			{
				title: 'Достаточно ли расширения браузера?',
				detail:
					'Расширение подключается к работающей службе Anda и не является самостоятельным облачным помощником. Сначала установите и настройте Anda, затем подключите расширение.'
			},
			{
				title: 'Продолжится ли работа после закрытия приложения?',
				detail:
					'Задачам нужны работающая служба Anda и доступ к настроенным сервисам. Закрытие клиента и остановка службы — разные действия. При выключенном компьютере задачи не выполняются.'
			}
		],
		final: {
			title: 'Найдите место для своего помощника.',
			body: 'Начните разговор, подключите инструменты и накапливайте полезный контекст в совместной работе.',
			install: 'Начать',
			docs: 'Читать документацию'
		},
		demo: {
			composer: {
				status: 'Локальная служба',
				placeholder: 'Сообщение для Anda…',
				turns: [
					{
						user: 'Отвечай коротко. Миа ведёт редизайн Atlas, релиз в пятницу.',
						reply: 'Принято. Запомню.',
						note: 'Сохранено 3 воспоминания'
					},
					{
						user: 'Составь чек-лист запуска по docs/launch.md.',
						reply: 'Готово. Файл checklist.md создан.',
						note: 'read_file · write_file'
					}
				]
			},
			memory: {
				kinds: ['Предпочтение', 'Проект', 'Человек', 'Решение', 'Событие'],
				items: [
					'Короткие и прямые ответы',
					'Редизайн Atlas',
					'Миа · руководит дизайном',
					'Релиз в пятницу',
					'Обзор запуска · 12 окт.'
				],
				revised: 'Релиз в понедельник',
				source: 'Из разговора',
				updated: 'Обновлено',
				linked: 'Связано'
			},
			action: {
				browser: 'Прочитано 3 источника',
				summary: 'Сводка',
				goal: 'Подготовить запуск Atlas',
				agents: ['Поиск', 'Черновик', 'Проверка'],
				routines: [
					{ when: 'Пн 09:00', what: 'Недельный отчёт' },
					{ when: 'Ежедневно 18:00', what: 'Сводка входящих' }
				],
				next: 'Следующий запуск'
			},
			control: {
				machine: 'Ваш компьютер',
				items: ['Настройки', 'Разговоры', 'Память'],
				model: 'Провайдер моделей',
				stays: 'Память хранится локально'
			}
		},
		install: {
			badge: 'Начало работы',
			title: 'Начните с того, что хотите сделать.',
			body: 'Установите Anda, подключите провайдера моделей и откройте удобный интерфейс. Текущие установщики включают CLI и программу запуска на поддерживаемых платформах.',
			detected: 'Обнаружена ОС: {os}',
			osLabel: 'Варианты установки в зависимости от операционной системы',
			copy: 'Копировать',
			copied: 'Скопировано',
			copyFailed: 'Копирование не удалось',
			copyAria: 'Копировать команду установки',
			options: {
				macos: {
					label: 'macOS',
					title: 'Anda Desktop',
					body: 'Скачайте Anda для Apple Silicon. Приложение установит командную строку anda, навыки и значок в строке меню, который поддерживает работу Anda после входа в систему.',
					primaryLabel: 'Скачать для Mac',
					href: macInstallerUrl,
					download: macInstallerFileName,
					command: desktopCaskCommand,
					commandLabel: 'Homebrew',
					note: 'Новые сборки для macOS доступны только для Apple Silicon; на Mac с Intel можно собрать из исходного кода. Только командная строка — используйте скрипт установки.',
					steps: ['Установить приложение', 'Настроить модель', 'Связать с браузером']
				},
				windows: {
					label: 'Windows',
					title: 'Anda Desktop',
					body: 'Скачайте установщик. Он установит Anda Desktop, командную строку anda, навыки и значок в трее, который поддерживает работу Anda после входа в систему.',
					primaryLabel: 'Скачать установщик',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: 'Значок в трее показывает состояние службы, устанавливает обновления, перезапускает службу, открывает логи и копирует токены для браузера.',
					steps: ['Запустить установку', 'Настроить модель', 'Связать с браузером']
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
			},
			requirements:
				'Нужен поддерживаемый провайдер моделей и, как правило, API-ключ. Использование моделей и внешних сервисов может оплачиваться отдельно.'
		}
	},
	ar: {
		meta: {
			title: 'Anda Bot — ذاكرة. عمل. القرار لك.',
			description:
				'مساعد ذكاء اصطناعي محلي يتذكر السياق المفيد ويعمل مع متصفحك وملفاتك وأدواتك. اختر نماذجك واحتفظ بالذاكرة على جهازك.'
		},
		nav: {
			memory: 'الذاكرة',
			action: 'العمل',
			control: 'القرار لك',
			surfaces: 'طرق الاستخدام',
			install: 'ابدأ',
			docs: 'التوثيق'
		},
		language: {
			label: 'اللغة'
		},
		hero: {
			badge: 'مساعد الذكاء الاصطناعي المحلي الخاص بك',
			title: ['ذاكرة.', 'عمل.', 'القرار لك.'],
			body: 'ينقل Anda Bot السياق المفيد من محادثاتك إلى العمل القادم. كلّفه بالمهام واربط أدواتك واستفد مما تراكم، مع نماذج تختارها وذاكرة محفوظة على جهازك.',
			primary: 'ابدأ الاستخدام',
			secondary: 'تعرّف على Anda',
			facts: ['مفتوح المصدر', 'ذاكرة محلية', 'نماذج من اختيارك']
		},
		pillars: [
			{
				title: 'تذكّر ما يهم',
				detail: 'احتفظ بالتفضيلات والسياق والقرارات للاستفادة منها في العمل لاحقاً.'
			},
			{
				title: 'حوّل الأفكار إلى عمل',
				detail: 'اعمل مع الويب والملفات المحلية والأدوات والمهام المجدولة.'
			},
			{
				title: 'احتفظ بحرية الاختيار',
				detail: 'اختر النماذج وأدر بياناتك المحلية بنفسك.'
			}
		],
		memory: {
			badge: '01 / الذاكرة',
			title: 'دع السياق المفيد يتراكم.',
			body: 'يمكن أن تصبح تفضيلاتك ومشاريعك وقراراتك المهمة سياقاً دائماً. ينظم Anda Brain المعلومات المفيدة من المحادثات ويسترجعها عند الحاجة.',
			features: [
				{
					title: 'تفضيلاتك في العمل',
					detail: 'انقل التفاصيل التي تحدد طريقة العمل المناسبة لك إلى المحادثات القادمة.'
				},
				{
					title: 'سياق مترابط',
					detail: 'اربط الأشخاص والمشاريع والأحداث والقرارات في رسم معرفي محلي.'
				},
				{
					title: 'معلومات تتغير',
					detail: 'أخبر Anda بما تغير ليدخل السياق الجديد في ذاكرته أيضاً.'
				}
			]
		},
		memoryLink: 'كيف تعمل الذاكرة',
		memoryPreview:
			'في الإصدار القادم: عرض مصادر الذاكرة ومراجعة التصحيحات أو الحذف للسجلات المدعومة.',
		previewLabel: 'الإصدار القادم',
		action: {
			badge: '02 / العمل',
			title: 'مساعد يشارك في العمل الفعلي.',
			body: 'اطرح الأسئلة أو استكشف المعلومات أو كلّفه بمهمة أطول. يمكن لـ Anda استخدام الأدوات التي تربطها ومواصلة العمل نحو هدف محدد.',
			features: [
				{
					title: 'التصفح والاستكشاف',
					detail: 'اقرأ الصفحات واجمع المعلومات وتفاعل مع المواقع عبر أدوات المتصفح.'
				},
				{
					title: 'الملفات والأدوات',
					detail: 'اقرأ الملفات المحلية واكتبها ونفّذ الأوامر ووسّع سير العمل بالمهارات وMCP.'
				},
				{
					title: 'متابعة المهام الطويلة',
					detail: 'حافظ على الهدف ونسّق الوكلاء الفرعيين وواصل العمل مع نمو السياق.'
				},
				{
					title: 'تنظيم الأعمال المتكررة',
					detail: 'جدول مهام لمرة واحدة أو بصورة دورية ما دامت خدمة Anda تعمل.'
				}
			]
		},
		control: {
			badge: '03 / القرار لك',
			title: 'غيّر النموذج واحتفظ بذاكرتك.',
			body: 'احتفظ بمساعد دائم دون ربط ذاكرته بحساب نموذج واحد.',
			features: [
				{
					title: 'بيانات على جهازك',
					detail: 'احتفظ بالإعدادات والمحادثات والذاكرة في مجلد Anda المحلي.'
				},
				{
					title: 'نماذج من اختيارك',
					detail: 'اربط مزودين مدعومين وغيّر النماذج مع الاحتفاظ بالذاكرة المحلية.'
				},
				{
					title: 'مفتوح بطبيعته',
					detail: 'اطّلع على المصدر واضبط الأدوات وكيّف المساعد مع طريقة عملك.'
				}
			]
		},
		dataNote:
			'التخزين المحلي لا يعني المعالجة دون اتصال. قد يُرسل المحتوى ذي الصلة إلى مزودي النماذج والخدمات التي تضبطها.',
		privacyLink: 'كيفية معالجة البيانات',
		sourceLink: 'عرض المصدر',
		surfaces: {
			badge: 'طرق استخدام Anda',
			title: 'مساعد في متناولك.',
			body: 'اختر الواجهة المناسبة لك. تتصل كل واجهة ببيئة Anda التي تضبطها.',
			items: [
				{
					title: 'المتصفح',
					detail:
						'استخدم Anda بجانب الصفحة التي تقرأها. اربط إضافة Chrome أو Edge بالخدمة المحلية.',
					linkLabel: 'إعداد الإضافة'
				},
				{
					title: 'الطرفية',
					detail: 'تحدث واختر مجلد العمل واستخدم الأدوات المحلية عبر سطر الأوامر وواجهة الطرفية.',
					linkLabel: 'الاستخدام في الطرفية'
				},
				{
					title: 'المراسلة',
					detail:
						'اربط Telegram أو WeChat أو Discord أو Lark/Feishu. تتبع صلاحيات الوصول إعدادات القنوات.',
					linkLabel: 'ربط قناة'
				},
				{
					title: 'تطبيق سطح المكتب',
					detail:
						'المحادثات والذاكرة والمتصفح والطرفية وGit في تطبيق سطح مكتب واحد، مع رمز في شريط النظام يُبقي Anda يعمل.',
					linkLabel: 'عن Anda Desktop'
				}
			]
		},
		faqTitle: 'قبل أن تبدأ',
		faq: [
			{
				title: 'هل أحتاج إلى إعداد خدمة ذاكرة منفصلة؟',
				detail:
					'تستخدم الذاكرة العادية إعدادات النماذج لديك. لا تحتاج إلى إعداد بيئة Brain المتقدمة أو ميزات التعلم للبدء.'
			},
			{
				title: 'هل تتم كل المعالجة على جهازي؟',
				detail:
					'تُحفظ الذاكرة وحالة التشغيل محلياً. وقد تعالج النماذج وخدمات الصوت والأدوات المحتوى خارج الجهاز وفق إعداداتك.'
			},
			{
				title: 'هل تكفي إضافة المتصفح وحدها؟',
				detail:
					'تتصل الإضافة بخدمة Anda قيد التشغيل وليست مساعداً سحابياً مستقلاً. ثبّت Anda واضبطه أولاً ثم اربط الإضافة.'
			},
			{
				title: 'هل يستمر العمل بعد إغلاق التطبيق؟',
				detail:
					'تحتاج المهام إلى استمرار تشغيل خدمة Anda وإمكانية الوصول إلى الخدمات المضبوطة. إغلاق الواجهة يختلف عن إيقاف الخدمة، ولا يمكن تنفيذ المهام عند إطفاء الكمبيوتر.'
			}
		],
		final: {
			title: 'افسح مكاناً لمساعدك الخاص.',
			body: 'ابدأ محادثة واربط أدواتك ودع السياق المفيد يتراكم من خلال العمل معاً.',
			install: 'ابدأ الاستخدام',
			docs: 'اقرأ التوثيق'
		},
		demo: {
			composer: {
				status: 'خدمة محلية',
				placeholder: 'اكتب رسالة إلى Anda…',
				turns: [
					{
						user: 'ردود قصيرة من فضلك. تصميم Atlas الجديد مع Mia، والإطلاق يوم الجمعة.',
						reply: 'حسناً، سأتذكر ذلك.',
						note: 'حُفظت 3 ذكريات'
					},
					{
						user: 'اكتب قائمة مهام للإطلاق من docs/launch.md.',
						reply: 'تم. ملف checklist.md جاهز.',
						note: 'read_file · write_file'
					}
				]
			},
			memory: {
				kinds: ['تفضيل', 'مشروع', 'شخص', 'قرار', 'حدث'],
				items: [
					'ردود قصيرة ومباشرة',
					'إعادة تصميم Atlas',
					'Mia · قيادة التصميم',
					'الإطلاق يوم الجمعة',
					'مراجعة الإطلاق · 12 أكتوبر'
				],
				revised: 'الإطلاق يوم الاثنين',
				source: 'من المحادثة',
				updated: 'تم التحديث',
				linked: 'مرتبط'
			},
			action: {
				browser: 'تمت قراءة 3 مصادر',
				summary: 'ملخص',
				goal: 'تحضير إطلاق Atlas',
				agents: ['بحث', 'صياغة', 'مراجعة'],
				routines: [
					{ when: 'الاثنين 09:00', what: 'تقرير أسبوعي' },
					{ when: 'يومياً 18:00', what: 'ملخص البريد' }
				],
				next: 'التشغيل التالي'
			},
			control: {
				machine: 'جهازك',
				items: ['الإعدادات', 'المحادثات', 'الذاكرة'],
				model: 'مزود النماذج',
				stays: 'الذاكرة محفوظة محلياً'
			}
		},
		install: {
			badge: 'ابدأ العمل',
			title: 'ابدأ بما تريد إنجازه.',
			body: 'ثبّت Anda واربط مزود نماذج وافتح واجهتك المفضلة. تتضمن برامج التثبيت الحالية سطر الأوامر والمشغّل على المنصات المدعومة.',
			detected: 'تم اكتشاف نظام التشغيل: {os}',
			osLabel: 'مسار التثبيت حسب نظام التشغيل المكتشف',
			copy: 'نسخ',
			copied: 'تم النسخ',
			copyFailed: 'فشل النسخ',
			copyAria: 'نسخ أمر التثبيت',
			options: {
				macos: {
					label: 'macOS',
					title: 'Anda Desktop',
					body: 'نزّل Anda لأجهزة Apple Silicon. يثبّت سطر أوامر anda والمهارات المختارة ورمزًا في شريط القوائم يُبقي Anda يعمل عند تسجيل الدخول.',
					primaryLabel: 'تنزيل لنظام Mac',
					href: macInstallerUrl,
					download: macInstallerFileName,
					command: desktopCaskCommand,
					commandLabel: 'Homebrew',
					note: 'إصدارات macOS الجديدة تدعم Apple Silicon فقط؛ يمكن لمستخدمي Mac بمعالج Intel البناء من المصدر. لسطر الأوامر فقط: استخدم سكريبت التثبيت.',
					steps: ['تثبيت التطبيق', 'إعداد النموذج', 'ربط المتصفح']
				},
				windows: {
					label: 'Windows',
					title: 'Anda Desktop',
					body: 'نزّل برنامج التثبيت. يثبّت Anda Desktop وسطر أوامر anda والمهارات المختارة ورمزًا في شريط النظام يُبقي Anda يعمل عند تسجيل الدخول.',
					primaryLabel: 'تنزيل برنامج التثبيت',
					href: windowsInstallerUrl,
					download: windowsInstallerFileName,
					note: 'يعرض الرمز حالة الخدمة، ويثبّت التحديثات، ويعيد تشغيل الخدمة، ويفتح السجلات، وينسخ رموز ربط المتصفح.',
					steps: ['تشغيل الإعداد', 'إعداد النموذج', 'ربط المتصفح']
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
			},
			requirements:
				'تحتاج إلى مزود نماذج مدعوم وإلى مفتاح API عادةً. قد تترتب رسوم منفصلة على استخدام النماذج والخدمات المرتبطة.'
		}
	}
};

export function isLocale(value: string | null | undefined): value is Locale {
	return Boolean(value && Object.hasOwn(localeMeta, value));
}
export function detectLocale(languages: readonly string[]): Locale {
	for (const language of languages) {
		const base = language.toLowerCase().split('-')[0];
		if (isLocale(base)) return base;
	}
	return fallbackLocale;
}
