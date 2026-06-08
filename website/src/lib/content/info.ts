import type { Locale } from './landing';

export type InfoPageKind = 'privacy' | 'terms' | 'support';

export type InfoItemCopy = {
	title: string;
	detail: string;
};

export type InfoSectionCopy = {
	title: string;
	body?: string[];
	items?: InfoItemCopy[];
};

export type InfoActionCopy = {
	label: string;
	href: string;
};

export type InfoPageCopy = {
	meta: {
		title: string;
		description: string;
	};
	eyebrow: string;
	title: string;
	intro: string;
	updated: string;
	sections: InfoSectionCopy[];
	actions?: InfoActionCopy[];
};

export type InfoLocaleCopy = {
	common: {
		home: string;
		docs: string;
		github: string;
		privacy: string;
		terms: string;
		support: string;
		languageLabel: string;
		updatedLabel: string;
		navigationLabel: string;
		moreLinks: string;
	};
	pages: Record<InfoPageKind, InfoPageCopy>;
};

const githubIssues = 'https://github.com/ldclabs/anda-bot/issues';
const githubDiscussions = 'https://github.com/ldclabs/anda-bot/discussions';
const docsUrl = 'https://docs.anda.bot';

export const infoCopy: Record<Locale, InfoLocaleCopy> = {
	en: {
		common: {
			home: 'Home',
			docs: 'Docs',
			github: 'GitHub',
			privacy: 'Privacy Policy',
			terms: 'Terms of Service',
			support: 'Support',
			languageLabel: 'Language',
			updatedLabel: 'Last updated',
			navigationLabel: 'Site navigation',
			moreLinks: 'More information'
		},
		pages: {
			privacy: {
				meta: {
					title: 'Privacy Policy - Anda Bot',
					description:
						'How the Anda Bot Chrome extension and website handle local settings, browser context, prompts, voice features, and model provider data.'
				},
				eyebrow: 'Privacy Policy',
				title: 'Privacy, local control, and browser data',
				intro:
					'Anda Bot is designed as a local-first agent bridge. The Chrome extension connects your browser to the Anda daemon you run, while the website explains and distributes the project.',
				updated: 'May 15, 2026',
				sections: [
					{
						title: 'What this policy covers',
						body: [
							'This policy covers the Anda Bot website and the Anda Bot Chrome extension. The extension is a side panel client for a local Anda daemon. The local daemon, your configured model providers, and any external services you connect may have their own data practices.',
							'The extension does not sell personal data. It exists to send your instructions and selected browser context to the local Anda runtime you configure.'
						]
					},
					{
						title: 'Data stored by the extension',
						items: [
							{
								title: 'Connection settings',
								detail:
									'The Gateway URL and Bearer token you paste into the settings panel are stored in Chrome local storage so the extension can reconnect to your local Anda daemon.'
							},
							{
								title: 'Browser session id',
								detail:
									'A stable browser session id is stored locally so Anda can keep one browser conversation thread as you switch tabs.'
							},
							{
								title: 'Conversation display state',
								detail:
									'The side panel may display conversation history returned by the local Anda daemon, but conversation records are managed by the daemon rather than by a remote extension service.'
							}
						]
					},
					{
						title: 'Browser context and permissions',
						body: [
							'When connected, the extension can register the current tab id, title, and URL with your local Anda daemon. If a task requires browser work, the daemon may ask the extension to list tabs, open or switch tabs, navigate, capture the visible tab, or run page actions such as reading content, clicking, typing, scrolling, and pressing keys.',
							'TTS permissions let Anda speak responses through Chrome when you choose voice playback. Voice capture or speech recognition only starts from user-facing voice controls and may depend on browser microphone permissions for the active page.'
						]
					},
					{
						title: 'Prompts, files, and model providers',
						body: [
							'Your prompts, selected attachments, generated transcripts, screenshots, page text, and tool results can be sent to the local Anda daemon. The daemon may then send relevant content to the model providers and services you configure in Anda.',
							'API keys and provider settings are controlled by your local Anda configuration. Review the privacy terms of each provider before sending sensitive information.'
						]
					},
					{
						title: 'Retention and control',
						items: [
							{
								title: 'Local extension data',
								detail:
									'You can clear the Gateway URL, token, and session data through Chrome extension storage or by removing the extension.'
							},
							{
								title: 'Anda runtime data',
								detail:
									'Anda stores runtime state, logs, channels, workspace files, and memory data in the local Anda home directory unless you configure a different location.'
							},
							{
								title: 'Website analytics',
								detail:
									'The current website code does not include a custom analytics tracker. Hosting infrastructure may still create standard operational logs.'
							}
						]
					},
					{
						title: 'Sensitive use',
						body: [
							'Do not send secrets, regulated records, confidential business data, or other sensitive content unless you understand where your local daemon, tools, model providers, and connected services will process it.',
							'Anda Bot is not intended for children under the age required by applicable law to use online services without parental consent.'
						]
					}
				],
				actions: [
					{ label: 'Open support', href: '/support' },
					{ label: 'GitHub issues', href: githubIssues }
				]
			},
			terms: {
				meta: {
					title: 'Terms of Service - Anda Bot',
					description:
						'Terms for using the Anda Bot website, Chrome extension, local runtime bridge, browser automation features, and open-source project resources.'
				},
				eyebrow: 'Terms of Service',
				title: 'Terms for using Anda Bot',
				intro:
					'These terms explain the responsibilities that come with using the website, Chrome extension, and local Anda runtime bridge.',
				updated: 'May 15, 2026',
				sections: [
					{
						title: 'Acceptance and scope',
						body: [
							'By using the website or Chrome extension, you agree to these terms. If you do not agree, do not use the website or extension.',
							'Anda Bot is open-source software and a local-first agent system. Some functionality depends on your local installation, model providers, browsers, operating system, and third-party tools.'
						]
					},
					{
						title: 'Your setup and accounts',
						items: [
							{
								title: 'Local daemon',
								detail:
									'You are responsible for installing, configuring, updating, and securing the local Anda Bot program and any home directory or workspace it uses.'
							},
							{
								title: 'API keys',
								detail:
									'You are responsible for your model provider accounts, API keys, usage costs, rate limits, and provider terms.'
							},
							{
								title: 'Extension token',
								detail:
									'Keep the Bearer token private. Anyone with access to it may be able to connect to your local Anda gateway while it is reachable.'
							}
						]
					},
					{
						title: 'Browser automation',
						body: [
							'The extension can help Anda interact with browser tabs and pages. You are responsible for reviewing actions before using them in accounts, administrative systems, purchases, financial workflows, production services, or other sensitive environments.',
							'Do not use Anda Bot to violate websites, terms of service, access controls, laws, or the rights of others.'
						]
					},
					{
						title: 'Content and output',
						body: [
							'You retain responsibility for the prompts, files, browser content, and other materials you provide. You are responsible for verifying generated output before relying on it.',
							'AI-generated output can be incomplete, incorrect, unsafe, or unsuitable for a specific purpose. Use professional judgment for legal, medical, financial, security, and operational decisions.'
						]
					},
					{
						title: 'Open-source license and availability',
						body: [
							'The Anda Bot source code is provided under the license in the repository. These terms do not replace the open-source license for the code.',
							'The website, extension, releases, documentation, and integrations may change, pause, or stop at any time. Features are provided as available and without a separate service-level commitment.'
						]
					},
					{
						title: 'No warranty and limitation of liability',
						body: [
							'To the maximum extent permitted by law, Anda Bot is provided as is, without warranties of any kind.',
							'To the maximum extent permitted by law, the project maintainers are not liable for indirect, incidental, special, consequential, or punitive damages, or for loss of data, profits, business, or goodwill.'
						]
					}
				],
				actions: [
					{ label: 'Read privacy policy', href: '/privacy' },
					{ label: 'GitHub repository', href: 'https://github.com/ldclabs/anda-bot' }
				]
			},
			support: {
				meta: {
					title: 'Support - Anda Bot',
					description:
						'Where to get help with Anda Bot installation, Chrome extension setup, browser token connection, local daemon issues, and bug reports.'
				},
				eyebrow: 'Support',
				title: 'Get help with Anda Bot',
				intro:
					'Anda Bot is an open-source local agent. The fastest path to support is usually to check setup, capture the exact error, and open a GitHub issue with enough detail to reproduce it.',
				updated: 'May 15, 2026',
				sections: [
					{
						title: 'Best support channels',
						items: [
							{
								title: 'Bug reports',
								detail:
									'Open a GitHub issue for reproducible extension, daemon, install, browser tool, or build problems.'
							},
							{
								title: 'Questions and ideas',
								detail:
									'Use GitHub discussions for usage questions, workflows, feature ideas, and community help.'
							},
							{
								title: 'Documentation',
								detail:
									'Use the docs and README for install commands, model provider configuration, Chrome token setup, Skills, channels, voice, and troubleshooting context.'
							}
						]
					},
					{
						title: 'Before opening an issue',
						items: [
							{
								title: 'Confirm local setup',
								detail:
									'Run anda from a terminal, confirm a model provider is configured, and make sure the daemon is reachable before testing the extension.'
							},
							{
								title: 'Regenerate the browser token',
								detail:
									'Run anda browser token --days 365 and paste both the Gateway URL and Bearer token into the side panel settings.'
							},
							{
								title: 'Check permissions',
								detail:
									'Chrome may block page injection, microphone access, file URLs, or extension actions on restricted pages. Try a normal https page when debugging.'
							}
						]
					},
					{
						title: 'Include this information',
						body: [
							'Please include your operating system, Anda Bot version, Chrome version, extension version, install method, the command you ran, the exact error message, and whether the issue happens on a fresh browser tab.',
							'Do not paste API keys, Bearer tokens, private prompts, confidential files, or sensitive screenshots into public issues.'
						]
					},
					{
						title: 'Security and privacy reports',
						body: [
							'For security-sensitive problems, avoid posting secrets or exploit details in a public issue. Open a minimal report first or use the repository security reporting options if available.',
							'If a token may have been exposed, revoke it by removing extension settings and generating a fresh browser token from the local Anda CLI.'
						]
					}
				],
				actions: [
					{ label: 'Open GitHub issues', href: githubIssues },
					{ label: 'Open discussions', href: githubDiscussions },
					{ label: 'Read docs', href: docsUrl }
				]
			}
		}
	},
	zh: {
		common: {
			home: '首页',
			docs: '文档',
			github: 'GitHub',
			privacy: '隐私政策',
			terms: '服务条款',
			support: '技术支持',
			languageLabel: '语言',
			updatedLabel: '最后更新于',
			navigationLabel: '站点导航',
			moreLinks: '更多信息'
		},
		pages: {
			privacy: {
				meta: {
					title: '隐私政策 - Anda Bot',
					description:
						'了解 Anda Bot Chrome 扩展和网站如何安全地处理本地设置、浏览器上下文、提示词、语音功能及大模型服务商的数据。'
				},
				eyebrow: '隐私政策',
				title: '隐私安全、本地控制与浏览器数据',
				intro:
					'Anda Bot 的核心理念是做本地优先的智能体桥梁。Chrome 扩展负责将浏览器连接至你自己运行的 Anda 后台进程，网站则用于展示和分发该开源项目。',
				updated: '2026 年 5 月 15 日',
				sections: [
					{
						title: '本政策适用范围',
						body: [
							'本政策适用于 Anda Bot 网站及 Anda Bot Chrome 扩展。该扩展是你本地 Anda 守护进程（Daemon）的侧边栏客户端。而你本地的守护进程、配置的模型服务商，以及连接的其他外部服务，可能遵循其各自的数据隐私条款。',
							'该扩展绝不会出售任何个人数据。它的唯一用途是将你的指令及你所选的浏览器上下文，透传给配置在本地的 Anda 运行环境。'
						]
					},
					{
						title: '扩展程序保存的数据',
						items: [
							{
								title: '连接凭证',
								detail:
									'你在设置面板中填写的 Gateway URL 和 Bearer Token 会被安全地保存在 Chrome 的本地存储中，以便扩展随时重新连接至你本地的 Anda 守护进程。'
							},
							{
								title: '浏览器会话 ID',
								detail:
									'扩展会在本地保存一个稳定的临时会话标识，以确保你在不同标签页之间切换时，Anda 仍能保持连贯的对话线索。'
							},
							{
								title: '会话显示状态',
								detail:
									'侧边栏会展示由本地 Anda 守护进程返回的会话记录，但这些数据全权由本地进程管理，扩展本身没有也不会依赖任何远程服务端。'
							}
						]
					},
					{
						title: '浏览器上下文与操作权限',
						body: [
							'连接成功后，扩展可将当前标签页的 ID、标题和 URL 登记到本机的 Anda 守护进程。如果某项任务涉及浏览器操作，守护进程可能会请求扩展获取标签页列表，执行页面切换、导航、可见区域截图，或者深度的页面交互（如：读取文本、点击、输入、滚动及按键）。',
							'在选择语音播放时，扩展需要 TTS 权限来通过 Chrome 朗读回答。语音录音或识别仅在你主动触发对应按钮时开启，并且可能会向浏览器请求当前页面的麦克风权限。'
						]
					},
					{
						title: '提示词、文件及模型服务商',
						body: [
							'你的提示词、所选附件、语音识别文本、截图、页面文字及工具调用结果均会被发送至本机的 Anda 守护进程。接着，守护进程可能会将相关信息提交给你所配置的大模型服务商或其他外部服务。',
							'API Key 和模型服务商设置均受本机的 Anda 配置管控。在发送敏感信息前，请务必了解每一家服务商的隐私政策。'
						]
					},
					{
						title: '数据保留与用户控制',
						items: [
							{
								title: '本地扩展数据',
								detail:
									'你可以随时通过清除 Chrome 扩展存储或直接卸载扩展，来彻底抹除 Gateway URL、Token 及会话数据。'
							},
							{
								title: 'Anda 运行时数据',
								detail:
									'除非特别指定了其他路径，Anda 会将运行时状态、日志、通信频道、工作区文件和所有的记忆数据统统保存在本地的 Anda home 目录。'
							},
							{
								title: '网站访问统计',
								detail:
									'当前网站代码未嵌入任何自定义的数据分析追踪器，不过底层的托管设施可能仍会生成标准的运行日志。'
							}
						]
					},
					{
						title: '关于敏感场景的提醒',
						body: [
							'请勿随意发送密码凭证、受监管记录、商业机密或其他机密信息，除非你非常明确并完全信任本地守护进程、引用的工具、模型服务商以及连接的外部服务将如何处理它们。',
							'Anda Bot 不适合未达法定年龄且未经监护人同意便使用类似在线服务的儿童。'
						]
					}
				],
				actions: [
					{ label: '获取技术支持', href: '/support' },
					{ label: '反馈 GitHub Issue', href: githubIssues }
				]
			},
			terms: {
				meta: {
					title: '服务条款 - Anda Bot',
					description:
						'规范使用 Anda Bot 网站、Chrome 扩展程序、本地运行时桥接、浏览器自动化及开源项目资源的服务条款。'
				},
				eyebrow: '服务条款',
				title: '使用 Anda Bot 的条款与责任',
				intro:
					'以下条款明晰了你在使用 Anda Bot 网站、Chrome 扩展及本地运行时桥接过程中的权责边界。',
				updated: '2026 年 5 月 15 日',
				sections: [
					{
						title: '条款接受与适用范围',
						body: [
							'访问网站或使用 Chrome 扩展即表示你同意并遵守本条款。若不同意，请立刻停止使用相关服务。',
							'Anda Bot 是一个开源且完全本地优先的智能体系统。其部分能力直接依赖于你所处的本地环境、模型服务商、浏览器、操作系统及第三方工具链。'
						]
					},
					{
						title: '属于用户的系统设定与账号责任',
						items: [
							{
								title: '本地守护进程',
								detail:
									'你需全权负责本机 Anda Bot 程序及其数据目录、工作区的安装、系统配置、更新维护和安全防护。'
							},
							{
								title: 'API 密钥',
								detail:
									'你需自行负责所选模型服务商的账户管理、API 密钥安全、使用资费、调用频率限制及遵守其服务商协议。'
							},
							{
								title: '扩展 Token',
								detail:
									'请务必妥善保管 Bearer token。凡持有该 token 的个体，在你的本地 Anda 网关暴露于网络时，均拥有连接并操作它的可能。'
							}
						]
					},
					{
						title: '关于浏览器自动化的风险声明',
						body: [
							'该扩展致力于辅助 Anda 完成基于浏览器及前端页面的自动化交互。若在个人账号、后台管理系统、线上购物、财务流程、关键生产环境及其它敏感系统中运行工作流，你必须严格审查前置操作并承担后果。',
							'严正声明：禁止利用 Anda Bot 违反任意站方规定、突破访问控制权限、触犯法律法规或侵犯他人合法权益。'
						]
					},
					{
						title: '内容的提供与输出校验',
						body: [
							'你需对所提供的提示词、本地文件、浏览器页面内容及所有其他素材承担完全责任，并在采纳生成结果前负责校验其准确性。',
							'AI 生成的内容可能残缺不全、存在事实错误、暗含安全隐患或不契合特定目标。涉及法律合规、医疗诊断、财务安全及关键业务等严肃决策中，请务必以人类专业人士的判断为准。'
						]
					},
					{
						title: '开源许可指引与服务的可用性',
						body: [
							'Anda Bot 的源代码基于仓库内附带的特定开源协议提供，本条款毫不妨碍也不替代该底层的开源许可保障。',
							'有关网站、扩展程序、发布版本、文档及软件集成等服务可能随时迭代、暂停或终止。各项服务均“按现状”提供，不包含独立的级别保证（SLA）。'
						]
					},
					{
						title: '免责声明与有限责任条款',
						body: [
							'在适用法律允许的最大限度内，Anda Bot 及所有关联组件严格“按现状”提供，我们不提供任何形式的明示或暗示担保。',
							'在适用法律允许的最大范畴内，即使已被提前告知风险，该项目维护团队绝不对任何间接的、偶然的、特殊的、继发的或惩罚性的损害负责。针对数据的意外毁损、预期利润骤减、商业机会丧失或商誉贬值等，项目组同样不承担赔偿义务。'
						]
					}
				],
				actions: [
					{ label: '阅读隐私政策', href: '/privacy' },
					{ label: '访问 GitHub 仓库', href: 'https://github.com/ldclabs/anda-bot' }
				]
			},
			support: {
				meta: {
					title: '技术支持 - Anda Bot',
					description:
						'快速解决 Anda Bot 安装、Chrome 扩展设置、本地服务连接问题的指南，学习提交高质量缺陷反馈的最佳实践。'
				},
				eyebrow: '技术支持',
				title: '获取 Anda Bot 的帮助',
				intro:
					'作为一款主打开源的本地智能体，排查故障的最快路径通常是优先检查本地设定、截取原生错误日志，随后提供可重现的详情并在 GitHub 上提交结构完善的 Issue。',
				updated: '2026 年 5 月 15 日',
				sections: [
					{
						title: '推荐的支持通道',
						items: [
							{
								title: '提交缺陷报告 (Bug)',
								detail:
									'针对在扩展、守护进程、安装环节、工具调用或构建中必然复现的异常问题，请提交 GitHub Issue。'
							},
							{
								title: '交流想法与疑问',
								detail:
									'涉及使用技巧、自动化设定、功能畅想和寻求社区经验分享等开放式话题，欢迎在 GitHub Discussions 交流。'
							},
							{
								title: '深度阅读官方手册',
								detail:
									'参考官方 Docs 及项目 README 以掌握安装命令、模型参数调优、安全 Token 打通、特定功能包（Skills）、语音对话交互及常见的排障手段。'
							}
						]
					},
					{
						title: '提报 Issue 前的自检建议',
						items: [
							{
								title: '验证本地环境就绪',
								detail:
									'在终端执行 anda，确保已正确配置了模型后端，并确认本地服务的端口通讯顺畅无阻，随后再使用浏览器扩展进行联调。'
							},
							{
								title: '重新签发鉴权 Token',
								detail:
									'尝试在终端运行 anda browser token --days 365 强制颁发新凭证。特别注意，Gateway URL 与 Bearer token 都必须准确无误地更新进侧边栏设置。'
							},
							{
								title: '规避浏览器防御机制',
								detail:
									'Chrome 可能因安全策略在受限站点阻截脚本注入、静音麦克风、禁用 file:// 本地协议等。调试时推荐用常用的独立 https 网页来交叉验证。'
							}
						]
					},
					{
						title: '提报所需的标准信息',
						body: [
							'工单时请尽可能包含：精确的操作系统版本、Anda Bot 发行版号、Chrome 内核版本、本扩展版本、部署途径、触发问题的对应指令及无错漏的报错回溯栈，并注明该现象在新的空白标签页中是否依旧存在。',
							'高度警惕：切勿在公开的工单环境贴入真实的 API 密钥对、Bearer 令牌、涉密提示词、受控制的配置文件或不慎包含私密数据的系统截屏。'
						]
					},
					{
						title: '安全漏洞的保密披露规范',
						body: [
							'对于极其敏感的安全问题或暴露用户隐私链路的漏洞事故，切勿在开源社区公开底层的秘钥构造和直接利用手法。请利用脱敏的方式发起简版报告，或使用仓库原生的安全预警专题通道（若存在）。',
							'倘若判断某项通信钥匙存在外泄风险，请雷厉风行地清空扩展内的参数设置，并通过执行本地 Anda 命令行管控终端生成全新的验证密钥来紧急止损。'
						]
					}
				],
				actions: [
					{ label: '提报 GitHub Issue', href: githubIssues },
					{ label: '参与 Discussions 讨论', href: githubDiscussions },
					{ label: '查阅完整文档', href: docsUrl }
				]
			}
		}
	},
	es: {
		common: {
			home: 'Inicio',
			docs: 'Docs',
			github: 'GitHub',
			privacy: 'Privacidad',
			terms: 'Términos',
			support: 'Soporte',
			languageLabel: 'Idioma',
			updatedLabel: 'Última actualización',
			navigationLabel: 'Navegación del sitio',
			moreLinks: 'Más información'
		},
		pages: {
			privacy: {
				meta: {
					title: 'Política de privacidad - Anda Bot',
					description:
						'Cómo la extensión de Chrome y el sitio de Anda Bot manejan ajustes locales, contexto del navegador, prompts, voz y proveedores de modelos.'
				},
				eyebrow: 'Política de privacidad',
				title: 'Privacidad, control local y datos del navegador',
				intro:
					'Anda Bot funciona como un puente local: la extensión conecta Chrome con el daemon de Anda que ejecutas en tu equipo.',
				updated: '15 de mayo de 2026',
				sections: [
					{
						title: 'Alcance',
						body: [
							'Esta política cubre el sitio web y la extensión de Chrome. El daemon local, los proveedores de modelos y otros servicios conectados pueden tener sus propias prácticas.',
							'La extensión no vende datos personales; envía tus instrucciones y el contexto elegido al runtime local de Anda que configuras.'
						]
					},
					{
						title: 'Datos guardados',
						items: [
							{
								title: 'Conexión',
								detail:
									'Gateway URL y Bearer token se guardan en almacenamiento local de Chrome para reconectar con tu daemon.'
							},
							{
								title: 'Sesión del navegador',
								detail: 'Un identificador local mantiene el mismo hilo mientras cambias de pestaña.'
							},
							{
								title: 'Historial mostrado',
								detail:
									'El panel puede mostrar conversaciones devueltas por el daemon; no las gestiona un servicio remoto de la extensión.'
							}
						]
					},
					{
						title: 'Permisos del navegador',
						body: [
							'La extensión puede registrar id, título y URL de la pestaña actual. Para una tarea puede listar, abrir o cambiar pestañas, navegar, capturar pantalla visible o ejecutar acciones de página como leer, hacer clic, escribir y desplazar.',
							'TTS y voz solo se usan cuando eliges funciones de voz y pueden requerir permisos del navegador.'
						]
					},
					{
						title: 'Prompts y proveedores',
						body: [
							'Prompts, adjuntos, transcripciones, capturas, texto de páginas y resultados de herramientas pueden enviarse al daemon local y, desde allí, a los proveedores que configures.',
							'Revisa los términos de privacidad de cada proveedor antes de enviar información sensible.'
						]
					},
					{
						title: 'Control',
						body: [
							'Puedes borrar datos locales de la extensión quitando la configuración o desinstalándola. Los datos del runtime de Anda se conservan en el directorio local de Anda salvo que configures otro lugar.'
						]
					}
				],
				actions: [
					{ label: 'Soporte', href: '/support' },
					{ label: 'GitHub issues', href: githubIssues }
				]
			},
			terms: {
				meta: {
					title: 'Términos de servicio - Anda Bot',
					description:
						'Términos para usar el sitio, la extensión, el puente local y las funciones de automatización de Anda Bot.'
				},
				eyebrow: 'Términos de servicio',
				title: 'Términos para usar Anda Bot',
				intro:
					'Estos términos describen responsabilidades al usar el sitio, la extensión y el runtime local.',
				updated: '15 de mayo de 2026',
				sections: [
					{
						title: 'Aceptación',
						body: [
							'Al usar el sitio o la extensión aceptas estos términos. Si no estás de acuerdo, no los uses.',
							'Anda Bot es software open source y algunas funciones dependen de tu instalación local, proveedores, navegador y herramientas.'
						]
					},
					{
						title: 'Tu configuración',
						items: [
							{
								title: 'Daemon local',
								detail: 'Eres responsable de instalar, configurar, actualizar y proteger Anda.'
							},
							{
								title: 'API keys',
								detail: 'Eres responsable de cuentas, costos, límites y términos de proveedores.'
							},
							{ title: 'Token', detail: 'Mantén privado el Bearer token de la extensión.' }
						]
					},
					{
						title: 'Automatización',
						body: [
							'Revisa acciones antes de usarlas en cuentas, compras, sistemas administrativos, servicios de producción o flujos sensibles.',
							'No uses Anda Bot para violar leyes, controles de acceso, derechos de terceros o términos de sitios web.'
						]
					},
					{
						title: 'Contenido y salida',
						body: [
							'Eres responsable del contenido que proporcionas y de verificar la salida generada antes de confiar en ella.',
							'La salida de IA puede ser incompleta o incorrecta. Usa juicio profesional en decisiones legales, médicas, financieras, de seguridad u operación.'
						]
					},
					{
						title: 'Licencia y garantía',
						body: [
							'El código se ofrece bajo la licencia del repositorio. En la máxima medida permitida por la ley, Anda Bot se proporciona tal cual, sin garantías.'
						]
					}
				],
				actions: [
					{ label: 'Privacidad', href: '/privacy' },
					{ label: 'GitHub', href: 'https://github.com/ldclabs/anda-bot' }
				]
			},
			support: {
				meta: {
					title: 'Soporte - Anda Bot',
					description:
						'Ayuda para instalación, extensión de Chrome, token del navegador, daemon local y reportes de errores.'
				},
				eyebrow: 'Soporte',
				title: 'Obtén ayuda con Anda Bot',
				intro:
					'Para soporte, verifica la configuración local, captura el error exacto y abre un issue con pasos reproducibles.',
				updated: '15 de mayo de 2026',
				sections: [
					{
						title: 'Canales',
						items: [
							{
								title: 'Errores',
								detail:
									'Usa GitHub issues para problemas reproducibles de extensión, daemon, instalación, navegador o build.'
							},
							{
								title: 'Preguntas',
								detail: 'Usa discussions para ideas, flujos de trabajo y ayuda de comunidad.'
							},
							{
								title: 'Documentación',
								detail:
									'Consulta docs y README para instalación, modelos, token, Skills, canales y voz.'
							}
						]
					},
					{
						title: 'Antes de reportar',
						items: [
							{
								title: 'Setup local',
								detail:
									'Ejecuta anda, confirma proveedor de modelo y prueba que el daemon responde.'
							},
							{
								title: 'Token',
								detail: 'Ejecuta anda browser token --days 365 y pega Gateway URL y Bearer token.'
							},
							{
								title: 'Permisos',
								detail:
									'Prueba en una página https normal; Chrome bloquea algunas acciones en páginas restringidas.'
							}
						]
					},
					{
						title: 'Incluye',
						body: [
							'Sistema operativo, versión de Anda, Chrome y extensión, método de instalación, comando usado y error exacto.',
							'No publiques API keys, tokens, prompts privados ni capturas sensibles.'
						]
					}
				],
				actions: [
					{ label: 'GitHub issues', href: githubIssues },
					{ label: 'Discussions', href: githubDiscussions },
					{ label: 'Docs', href: docsUrl }
				]
			}
		}
	},
	fr: {
		common: {
			home: 'Accueil',
			docs: 'Docs',
			github: 'GitHub',
			privacy: 'Confidentialité',
			terms: 'Conditions',
			support: 'Support',
			languageLabel: 'Langue',
			updatedLabel: 'Dernière mise à jour',
			navigationLabel: 'Navigation du site',
			moreLinks: 'Plus d’informations'
		},
		pages: {
			privacy: {
				meta: {
					title: 'Politique de confidentialité - Anda Bot',
					description:
						'Comment l’extension Chrome et le site Anda Bot traitent les réglages locaux, le contexte du navigateur, les prompts, la voix et les fournisseurs de modèles.'
				},
				eyebrow: 'Politique de confidentialité',
				title: 'Confidentialité, contrôle local et données du navigateur',
				intro:
					'Anda Bot est un pont local entre Chrome et le daemon Anda que vous exécutez sur votre ordinateur.',
				updated: '15 mai 2026',
				sections: [
					{
						title: 'Portée',
						body: [
							'Cette politique couvre le site web et l’extension Chrome. Le daemon local, les fournisseurs de modèles et les services connectés peuvent avoir leurs propres pratiques.',
							'L’extension ne vend pas de données personnelles; elle transmet vos instructions et le contexte choisi au runtime Anda local.'
						]
					},
					{
						title: 'Données enregistrées',
						items: [
							{
								title: 'Connexion',
								detail:
									'Gateway URL et Bearer token sont stockés localement dans Chrome pour reconnecter le daemon.'
							},
							{
								title: 'Session navigateur',
								detail:
									'Un identifiant local garde le même fil de navigateur lorsque vous changez d’onglet.'
							},
							{
								title: 'Historique affiché',
								detail:
									'Le panneau peut afficher les conversations renvoyées par le daemon, sans service distant dédié à l’extension.'
							}
						]
					},
					{
						title: 'Permissions navigateur',
						body: [
							'L’extension peut enregistrer l’id, le titre et l’URL de l’onglet actif. Pour une tâche, elle peut lister, ouvrir ou changer d’onglets, naviguer, capturer l’écran visible ou exécuter des actions de page.',
							'Les fonctions TTS et voix ne sont utilisées que lorsque vous choisissez des contrôles vocaux et peuvent demander des permissions du navigateur.'
						]
					},
					{
						title: 'Prompts et fournisseurs',
						body: [
							'Prompts, pièces jointes, transcriptions, captures, texte de page et résultats d’outils peuvent être envoyés au daemon local puis aux fournisseurs configurés.',
							'Vérifiez les politiques de chaque fournisseur avant d’envoyer des informations sensibles.'
						]
					},
					{
						title: 'Contrôle',
						body: [
							'Vous pouvez supprimer les données locales de l’extension en effaçant les réglages ou en la désinstallant. Les données du runtime restent dans le dossier local d’Anda sauf configuration contraire.'
						]
					}
				],
				actions: [
					{ label: 'Support', href: '/support' },
					{ label: 'GitHub issues', href: githubIssues }
				]
			},
			terms: {
				meta: {
					title: 'Conditions d’utilisation - Anda Bot',
					description:
						'Conditions d’utilisation du site, de l’extension, du pont local et de l’automatisation navigateur.'
				},
				eyebrow: 'Conditions d’utilisation',
				title: 'Conditions pour utiliser Anda Bot',
				intro:
					'Ces conditions expliquent vos responsabilités lors de l’utilisation du site, de l’extension et du runtime local.',
				updated: '15 mai 2026',
				sections: [
					{
						title: 'Acceptation',
						body: [
							'En utilisant le site ou l’extension, vous acceptez ces conditions. Sinon, ne les utilisez pas.',
							'Anda Bot est open source et certaines fonctions dépendent de votre installation locale, de vos fournisseurs, du navigateur et des outils.'
						]
					},
					{
						title: 'Votre configuration',
						items: [
							{
								title: 'Daemon local',
								detail:
									'Vous êtes responsable de l’installation, de la configuration, des mises à jour et de la sécurité d’Anda.'
							},
							{
								title: 'Clés API',
								detail:
									'Vous êtes responsable des comptes, coûts, limites et conditions des fournisseurs.'
							},
							{ title: 'Token', detail: 'Gardez privé le Bearer token de l’extension.' }
						]
					},
					{
						title: 'Automatisation',
						body: [
							'Vérifiez les actions avant de les utiliser dans des comptes, achats, systèmes d’administration, services de production ou contextes sensibles.',
							'N’utilisez pas Anda Bot pour violer la loi, des contrôles d’accès, les droits d’autrui ou les conditions de sites web.'
						]
					},
					{
						title: 'Contenu et sortie',
						body: [
							'Vous êtes responsable du contenu fourni et de la vérification des résultats générés.',
							'La sortie IA peut être incomplète ou incorrecte. Utilisez un jugement professionnel pour les décisions sensibles.'
						]
					},
					{
						title: 'Licence et garantie',
						body: [
							'Le code est fourni sous la licence du dépôt. Dans la mesure permise par la loi, Anda Bot est fourni tel quel, sans garantie.'
						]
					}
				],
				actions: [
					{ label: 'Confidentialité', href: '/privacy' },
					{ label: 'GitHub', href: 'https://github.com/ldclabs/anda-bot' }
				]
			},
			support: {
				meta: {
					title: 'Support - Anda Bot',
					description:
						'Aide pour installation, extension Chrome, token navigateur, daemon local et rapports de bugs.'
				},
				eyebrow: 'Support',
				title: 'Obtenir de l’aide avec Anda Bot',
				intro:
					'Vérifiez la configuration locale, notez l’erreur exacte et ouvrez un issue reproductible sur GitHub.',
				updated: '15 mai 2026',
				sections: [
					{
						title: 'Canaux',
						items: [
							{
								title: 'Bugs',
								detail:
									'Utilisez GitHub issues pour les problèmes reproductibles d’extension, daemon, installation, navigateur ou build.'
							},
							{
								title: 'Questions',
								detail: 'Utilisez discussions pour les idées, workflows et aide communautaire.'
							},
							{
								title: 'Documentation',
								detail:
									'Consultez docs et README pour installation, modèles, token, Skills, canaux et voix.'
							}
						]
					},
					{
						title: 'Avant de signaler',
						items: [
							{
								title: 'Setup local',
								detail: 'Exécutez anda, vérifiez le fournisseur de modèle et testez le daemon.'
							},
							{
								title: 'Token',
								detail:
									'Exécutez anda browser token --days 365 et collez Gateway URL et Bearer token.'
							},
							{
								title: 'Permissions',
								detail:
									'Testez sur une page https normale; Chrome bloque certaines pages restreintes.'
							}
						]
					},
					{
						title: 'Inclure',
						body: [
							'Système, versions d’Anda, Chrome et extension, méthode d’installation, commande lancée et erreur exacte.',
							'Ne publiez pas de clés API, tokens, prompts privés ou captures sensibles.'
						]
					}
				],
				actions: [
					{ label: 'GitHub issues', href: githubIssues },
					{ label: 'Discussions', href: githubDiscussions },
					{ label: 'Docs', href: docsUrl }
				]
			}
		}
	},
	ru: {
		common: {
			home: 'Главная',
			docs: 'Документация',
			github: 'GitHub',
			privacy: 'Конфиденциальность',
			terms: 'Условия',
			support: 'Поддержка',
			languageLabel: 'Язык',
			updatedLabel: 'Обновлено',
			navigationLabel: 'Навигация сайта',
			moreLinks: 'Дополнительно'
		},
		pages: {
			privacy: {
				meta: {
					title: 'Политика конфиденциальности - Anda Bot',
					description:
						'Как расширение Chrome и сайт Anda Bot обрабатывают локальные настройки, контекст браузера, запросы, голос и провайдеров моделей.'
				},
				eyebrow: 'Политика конфиденциальности',
				title: 'Конфиденциальность, локальный контроль и данные браузера',
				intro:
					'Anda Bot работает как локальный мост между Chrome и daemon Anda, который запущен на вашем компьютере.',
				updated: '15 мая 2026 г.',
				sections: [
					{
						title: 'Область действия',
						body: [
							'Политика относится к сайту и расширению Chrome. Локальный daemon, провайдеры моделей и подключенные сервисы могут иметь собственные правила.',
							'Расширение не продает персональные данные; оно передает ваши инструкции и выбранный контекст в локальный runtime Anda.'
						]
					},
					{
						title: 'Сохраняемые данные',
						items: [
							{
								title: 'Подключение',
								detail:
									'Gateway URL и Bearer token хранятся локально в Chrome для повторного подключения к daemon.'
							},
							{
								title: 'Сессия браузера',
								detail: 'Локальный идентификатор помогает сохранять один поток при смене вкладок.'
							},
							{
								title: 'История',
								detail:
									'Панель может показывать разговоры, возвращенные daemon; удаленный сервис расширения ими не управляет.'
							}
						]
					},
					{
						title: 'Разрешения браузера',
						body: [
							'Расширение может передавать id, заголовок и URL активной вкладки. Для задачи оно может работать с вкладками, навигацией, видимым скриншотом и действиями на странице.',
							'TTS и голос используются только при выборе голосовых функций и могут требовать разрешения браузера.'
						]
					},
					{
						title: 'Запросы и провайдеры',
						body: [
							'Запросы, вложения, транскрипты, скриншоты, текст страниц и результаты инструментов могут отправляться локальному daemon и затем выбранным провайдерам.',
							'Проверьте политики провайдеров перед отправкой чувствительных данных.'
						]
					},
					{
						title: 'Контроль',
						body: [
							'Локальные данные расширения можно удалить через настройки Chrome или удалив расширение. Данные runtime хранятся в локальной директории Anda, если не задано другое место.'
						]
					}
				],
				actions: [
					{ label: 'Поддержка', href: '/support' },
					{ label: 'GitHub issues', href: githubIssues }
				]
			},
			terms: {
				meta: {
					title: 'Условия использования - Anda Bot',
					description:
						'Условия для сайта, расширения, локального моста и автоматизации браузера Anda Bot.'
				},
				eyebrow: 'Условия использования',
				title: 'Условия использования Anda Bot',
				intro:
					'Эти условия описывают ответственность при использовании сайта, расширения и локального runtime.',
				updated: '15 мая 2026 г.',
				sections: [
					{
						title: 'Принятие',
						body: [
							'Используя сайт или расширение, вы принимаете эти условия. Если не согласны, не используйте их.',
							'Anda Bot является open source; часть функций зависит от локальной установки, провайдеров, браузера и инструментов.'
						]
					},
					{
						title: 'Ваша настройка',
						items: [
							{
								title: 'Локальный daemon',
								detail: 'Вы отвечаете за установку, настройку, обновление и защиту Anda.'
							},
							{
								title: 'API keys',
								detail: 'Вы отвечаете за аккаунты провайдеров, расходы, лимиты и условия.'
							},
							{ title: 'Token', detail: 'Храните Bearer token расширения в тайне.' }
						]
					},
					{
						title: 'Автоматизация',
						body: [
							'Проверяйте действия перед использованием в аккаунтах, покупках, админ-системах, production-сервисах или чувствительных средах.',
							'Не используйте Anda Bot для нарушения законов, контроля доступа, прав других лиц или правил сайтов.'
						]
					},
					{
						title: 'Контент и вывод',
						body: [
							'Вы отвечаете за предоставленный контент и проверку сгенерированных результатов.',
							'AI-вывод может быть неполным или неверным. Используйте профессиональное суждение для важных решений.'
						]
					},
					{
						title: 'Лицензия и гарантии',
						body: [
							'Код предоставляется по лицензии репозитория. В пределах закона Anda Bot предоставляется как есть, без гарантий.'
						]
					}
				],
				actions: [
					{ label: 'Конфиденциальность', href: '/privacy' },
					{ label: 'GitHub', href: 'https://github.com/ldclabs/anda-bot' }
				]
			},
			support: {
				meta: {
					title: 'Поддержка - Anda Bot',
					description:
						'Помощь с установкой, расширением Chrome, browser token, локальным daemon и баг-репортами.'
				},
				eyebrow: 'Поддержка',
				title: 'Получить помощь по Anda Bot',
				intro:
					'Проверьте локальную настройку, сохраните точную ошибку и откройте issue с шагами воспроизведения.',
				updated: '15 мая 2026 г.',
				sections: [
					{
						title: 'Каналы',
						items: [
							{
								title: 'Ошибки',
								detail:
									'Используйте GitHub issues для воспроизводимых проблем расширения, daemon, установки, браузера или сборки.'
							},
							{
								title: 'Вопросы',
								detail: 'Используйте discussions для идей, workflow и помощи сообщества.'
							},
							{
								title: 'Документация',
								detail:
									'Смотрите docs и README для установки, моделей, token, Skills, каналов и голоса.'
							}
						]
					},
					{
						title: 'Перед отчетом',
						items: [
							{
								title: 'Локальная настройка',
								detail: 'Запустите anda, проверьте провайдера модели и доступность daemon.'
							},
							{
								title: 'Token',
								detail:
									'Выполните anda browser token --days 365 и вставьте Gateway URL и Bearer token.'
							},
							{
								title: 'Разрешения',
								detail:
									'Проверьте на обычной https-странице; Chrome блокирует некоторые ограниченные страницы.'
							}
						]
					},
					{
						title: 'Укажите',
						body: [
							'ОС, версии Anda, Chrome и расширения, способ установки, команду и точный текст ошибки.',
							'Не публикуйте API keys, tokens, приватные prompts или чувствительные скриншоты.'
						]
					}
				],
				actions: [
					{ label: 'GitHub issues', href: githubIssues },
					{ label: 'Discussions', href: githubDiscussions },
					{ label: 'Docs', href: docsUrl }
				]
			}
		}
	},
	ar: {
		common: {
			home: 'الرئيسية',
			docs: 'الوثائق',
			github: 'GitHub',
			privacy: 'الخصوصية',
			terms: 'الشروط',
			support: 'الدعم',
			languageLabel: 'اللغة',
			updatedLabel: 'آخر تحديث',
			navigationLabel: 'تنقل الموقع',
			moreLinks: 'معلومات إضافية'
		},
		pages: {
			privacy: {
				meta: {
					title: 'سياسة الخصوصية - Anda Bot',
					description:
						'كيف يتعامل موقع Anda Bot وإضافة Chrome مع الإعدادات المحلية وسياق المتصفح والمطالبات والصوت ومزوّدي النماذج.'
				},
				eyebrow: 'سياسة الخصوصية',
				title: 'الخصوصية والتحكم المحلي وبيانات المتصفح',
				intro: 'Anda Bot يعمل كجسر محلي بين Chrome وdaemon Anda الذي تشغله على جهازك.',
				updated: '15 مايو 2026',
				sections: [
					{
						title: 'النطاق',
						body: [
							'تنطبق هذه السياسة على الموقع وإضافة Chrome. قد يكون للdaemon المحلي ومزوّدي النماذج والخدمات المتصلة ممارسات خاصة بهم.',
							'الإضافة لا تبيع البيانات الشخصية؛ هي ترسل تعليماتك والسياق الذي تختاره إلى runtime Anda المحلي.'
						]
					},
					{
						title: 'البيانات المخزنة',
						items: [
							{
								title: 'الاتصال',
								detail:
									'يتم حفظ Gateway URL وBearer token محلياً في Chrome لإعادة الاتصال بالdaemon.'
							},
							{
								title: 'جلسة المتصفح',
								detail: 'معرّف محلي يحافظ على نفس مسار المحادثة عند تبديل التبويبات.'
							},
							{
								title: 'السجل المعروض',
								detail:
									'قد تعرض اللوحة محادثات يعيدها الdaemon، ولا تديرها خدمة بعيدة خاصة بالإضافة.'
							}
						]
					},
					{
						title: 'أذونات المتصفح',
						body: [
							'قد تسجل الإضافة id وعنوان وURL التبويب الحالي. وللمهام يمكنها إدارة التبويبات والتنقل ولقطة الشاشة المرئية وتنفيذ إجراءات الصفحة.',
							'تستخدم وظائف TTS والصوت فقط عندما تختار عناصر التحكم الصوتية وقد تتطلب أذونات المتصفح.'
						]
					},
					{
						title: 'المطالبات والمزوّدون',
						body: [
							'قد تُرسل المطالبات والمرفقات والتفريغات واللقطات ونص الصفحات ونتائج الأدوات إلى الdaemon المحلي ثم إلى المزوّدين الذين تضبطهم.',
							'راجع سياسات كل مزوّد قبل إرسال معلومات حساسة.'
						]
					},
					{
						title: 'التحكم',
						body: [
							'يمكن حذف بيانات الإضافة المحلية من إعدادات Chrome أو بإزالة الإضافة. تبقى بيانات runtime في دليل Anda المحلي ما لم تضبط موقعاً آخر.'
						]
					}
				],
				actions: [
					{ label: 'الدعم', href: '/support' },
					{ label: 'GitHub issues', href: githubIssues }
				]
			},
			terms: {
				meta: {
					title: 'شروط الخدمة - Anda Bot',
					description: 'شروط استخدام الموقع والإضافة والجسر المحلي وأتمتة المتصفح في Anda Bot.'
				},
				eyebrow: 'شروط الخدمة',
				title: 'شروط استخدام Anda Bot',
				intro: 'توضح هذه الشروط مسؤولياتك عند استخدام الموقع والإضافة وruntime المحلي.',
				updated: '15 مايو 2026',
				sections: [
					{
						title: 'القبول',
						body: [
							'باستخدام الموقع أو الإضافة فإنك توافق على هذه الشروط. إذا لم توافق، فلا تستخدمهما.',
							'Anda Bot برنامج مفتوح المصدر وبعض الوظائف تعتمد على تثبيتك المحلي والمزوّدين والمتصفح والأدوات.'
						]
					},
					{
						title: 'إعدادك',
						items: [
							{ title: 'Daemon محلي', detail: 'أنت مسؤول عن تثبيت Anda وضبطه وتحديثه وتأمينه.' },
							{
								title: 'API keys',
								detail: 'أنت مسؤول عن حسابات المزوّدين والتكاليف والحدود والشروط.'
							},
							{ title: 'Token', detail: 'حافظ على Bearer token الخاص بالإضافة بسرية.' }
						]
					},
					{
						title: 'الأتمتة',
						body: [
							'راجع الإجراءات قبل استخدامها في الحسابات أو المشتريات أو أنظمة الإدارة أو الخدمات الإنتاجية أو البيئات الحساسة.',
							'لا تستخدم Anda Bot لانتهاك القوانين أو ضوابط الوصول أو حقوق الآخرين أو شروط المواقع.'
						]
					},
					{
						title: 'المحتوى والمخرجات',
						body: [
							'أنت مسؤول عن المحتوى الذي تقدمه وعن التحقق من المخرجات قبل الاعتماد عليها.',
							'قد تكون مخرجات الذكاء الاصطناعي ناقصة أو خاطئة. استخدم حكماً مهنياً في القرارات الحساسة.'
						]
					},
					{
						title: 'الترخيص والضمان',
						body: [
							'يقدم الكود بموجب ترخيص المستودع. إلى أقصى حد يسمح به القانون، يقدم Anda Bot كما هو ودون ضمانات.'
						]
					}
				],
				actions: [
					{ label: 'الخصوصية', href: '/privacy' },
					{ label: 'GitHub', href: 'https://github.com/ldclabs/anda-bot' }
				]
			},
			support: {
				meta: {
					title: 'الدعم - Anda Bot',
					description:
						'مساعدة في التثبيت وإضافة Chrome وbrowser token والdaemon المحلي وتقارير الأخطاء.'
				},
				eyebrow: 'الدعم',
				title: 'احصل على مساعدة مع Anda Bot',
				intro: 'تحقق من الإعداد المحلي وسجل الخطأ بدقة وافتح issue بخطوات قابلة لإعادة الإنتاج.',
				updated: '15 مايو 2026',
				sections: [
					{
						title: 'القنوات',
						items: [
							{
								title: 'الأخطاء',
								detail:
									'استخدم GitHub issues للمشكلات القابلة لإعادة الإنتاج في الإضافة أو daemon أو التثبيت أو المتصفح أو البناء.'
							},
							{
								title: 'الأسئلة',
								detail: 'استخدم discussions للأفكار وسير العمل ومساعدة المجتمع.'
							},
							{
								title: 'الوثائق',
								detail: 'راجع docs وREADME للتثبيت والنماذج وtoken وSkills والقنوات والصوت.'
							}
						]
					},
					{
						title: 'قبل الإبلاغ',
						items: [
							{
								title: 'الإعداد المحلي',
								detail: 'شغل anda وتحقق من مزوّد النموذج وإمكانية الوصول إلى daemon.'
							},
							{
								title: 'Token',
								detail: 'شغل anda browser token --days 365 والصق Gateway URL وBearer token.'
							},
							{
								title: 'الأذونات',
								detail: 'جرّب صفحة https عادية؛ Chrome يحظر بعض الصفحات المقيدة.'
							}
						]
					},
					{
						title: 'أرفق',
						body: [
							'نظام التشغيل وإصدارات Anda وChrome والإضافة وطريقة التثبيت والأمر ونص الخطأ الدقيق.',
							'لا تنشر API keys أو tokens أو prompts خاصة أو لقطات حساسة.'
						]
					}
				],
				actions: [
					{ label: 'GitHub issues', href: githubIssues },
					{ label: 'Discussions', href: githubDiscussions },
					{ label: 'Docs', href: docsUrl }
				]
			}
		}
	}
};
