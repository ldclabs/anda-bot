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
								title: '对话显示状态',
								detail:
									'侧边栏会展示由本地 Anda 守护进程返回的对话记录，但这些数据全权由本地进程管理，扩展本身没有也不会依赖任何远程服务端。'
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
			privacy: 'Política de privacidad',
			terms: 'Términos de servicio',
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
						'Cómo la extensión de Chrome y el sitio web de Anda Bot manejan la configuración local, el contexto del navegador, los prompts, las funciones de voz y los datos de los proveedores de modelos.'
				},
				eyebrow: 'Política de privacidad',
				title: 'Privacidad, control local y datos del navegador',
				intro:
					'Anda Bot está diseñado como un puente de agente local-first. La extensión de Chrome conecta su navegador con el daemon de Anda que ejecuta, mientras que el sitio web explica y distribuye el proyecto.',
				updated: '15 de mayo de 2026',
				sections: [
					{
						title: 'Qué cubre esta política',
						body: [
							'Esta política cubre el sitio web de Anda Bot y la extensión de Chrome de Anda Bot. La extensión es un cliente de panel lateral para un daemon local de Anda. El daemon local, los proveedores de modelos configurados y cualquier servicio externo que conecte pueden tener sus propias prácticas de datos.',
							'La extensión no vende datos personales. Existe para enviar sus instrucciones y el contexto seleccionado del navegador al runtime local de Anda que configure.'
						]
					},
					{
						title: 'Datos almacenados por la extensión',
						items: [
							{
								title: 'Configuración de conexión',
								detail:
									'La Gateway URL y el Bearer token que pega en el panel de configuración se almacenan en el almacenamiento local de Chrome para que la extensión pueda reconectarse a su daemon local de Anda.'
							},
							{
								title: 'ID de sesión del navegador',
								detail:
									'Se almacena localmente un ID de sesión de navegador estable para que Anda pueda mantener un hilo de conversación de navegador mientras cambia de pestaña.'
							},
							{
								title: 'Estado de visualización de la conversación',
								detail:
									'El panel lateral puede mostrar el historial de conversación devuelto por el daemon local de Anda, pero los registros de conversación son gestionados por el daemon en lugar de por un servicio de extensión remoto.'
							}
						]
					},
					{
						title: 'Contexto y permisos del navegador',
						body: [
							'Cuando está conectada, la extensión puede registrar el ID de la pestaña actual, el título y la URL con su daemon local de Anda. Si una tarea requiere trabajo en el navegador, el daemon puede pedirle a la extensión que enumere pestañas, abra o cambie pestañas, navegue, capture la pestaña visible o ejecute acciones de la página como leer contenido, hacer clic, escribir, desplazarse y presionar teclas.',
							'Los permisos de TTS permiten que Anda pronuncie respuestas a través de Chrome cuando elige la reproducción de voz. La captura de voz o el reconocimiento de voz solo se inician desde los controles de voz orientados al usuario y pueden depender de los permisos del micrófono del navegador para la página activa.'
						]
					},
					{
						title: 'Prompts, archivos y proveedores de modelos',
						body: [
							'Sus prompts, archivos adjuntos seleccionados, transcripciones generadas, capturas de pantalla, texto de la página y resultados de herramientas pueden enviarse al daemon local de Anda. El daemon puede enviar el contenido relevante a los proveedores de modelos y servicios que configure en Anda.',
							'Las claves API y la configuración del proveedor están controladas por su configuración local de Anda. Revise los términos de privacidad de cada proveedor antes de enviar información sensible.'
						]
					},
					{
						title: 'Retención y control',
						items: [
							{
								title: 'Datos de la extensión local',
								detail:
									'Puede borrar la Gateway URL, el token y los datos de sesión a través del almacenamiento de la extensión de Chrome o eliminando la extensión.'
							},
							{
								title: 'Datos de ejecución de Anda',
								detail:
									'Anda almacena el estado de ejecución, registros, canales, archivos del espacio de trabajo y datos de memoria en el directorio de inicio local de Anda, a menos que configure una ubicación diferente.'
							},
							{
								title: 'Estadísticas del sitio web',
								detail:
									'El código actual del sitio web no incluye un rastreador de análisis personalizado. La infraestructura de alojamiento aún puede crear registros operativos estándar.'
							}
						]
					},
					{
						title: 'Uso sensible',
						body: [
							'No envíe secretos, registros regulados, datos comerciales confidenciales u otro contenido sensible a menos que comprenda dónde lo procesarán su daemon local, herramientas, proveedores de modelos y servicios conectados.',
							'Anda Bot no está destinado a niños menores de la edad requerida por la ley aplicable para usar servicios en línea sin el consentimiento de los padres.'
						]
					}
				],
				actions: [
					{ label: 'Abrir soporte', href: '/support' },
					{ label: 'Problemas de GitHub', href: githubIssues }
				]
			},
			terms: {
				meta: {
					title: 'Términos de servicio - Anda Bot',
					description:
						'Términos para usar el sitio web de Anda Bot, la extensión de Chrome, el puente de ejecución local, las funciones de automatización del navegador y los recursos del proyecto de código abierto.'
				},
				eyebrow: 'Términos de servicio',
				title: 'Términos para usar Anda Bot',
				intro:
					'Estos términos explican las responsabilidades que conlleva el uso del sitio web, la extensión de Chrome y el puente de ejecución local de Anda.',
				updated: '15 de mayo de 2026',
				sections: [
					{
						title: 'Aceptación y alcance',
						body: [
							'Al usar el sitio web o la extensión de Chrome, acepta estos términos. Si no está de acuerdo, no use el sitio web ni la extensión.',
							'Anda Bot es software de código abierto y un sistema de agentes local-first. Algunas funciones dependen de su instalación local, proveedores de modelos, navegadores, sistema operativo y herramientas de terceros.'
						]
					},
					{
						title: 'Su configuración y cuentas',
						items: [
							{
								title: 'Daemon local',
								detail:
									'Usted es responsable de instalar, configurar, actualizar y proteger el programa local Anda Bot y cualquier directorio de inicio o espacio de trabajo que utilice.'
							},
							{
								title: 'Claves API',
								detail:
									'Usted es responsable de sus cuentas de proveedores de modelos, claves API, costos de uso, límites de velocidad y términos del proveedor.'
							},
							{
								title: 'Token de la extensión',
								detail:
									'Mantenga el Bearer token privado. Cualquier persona con acceso a él puede conectarse a su puerta de enlace local de Anda mientras sea accesible.'
							}
						]
					},
					{
						title: 'Automatización del navegador',
						body: [
							'La extensión puede ayudar a Anda a interactuar con pestañas y páginas del navegador. Usted es responsable de revisar las acciones antes de usarlas en cuentas, sistemas administrativos, compras, flujos de trabajo financieros, servicios de producción u otros entornos sensibles.',
							'No use Anda Bot para violar sitios web, términos de servicio, controles de acceso, leyes o los derechos de otros.'
						]
					},
					{
						title: 'Contenido y resultados',
						body: [
							'Usted conserva la responsabilidad de los prompts, archivos, contenido del navegador y otros materiales que proporcione. Es responsable de verificar los resultados generados antes de confiar en ellos.',
							'Los resultados generados por IA pueden ser incompletos, incorrectos, inseguros o inadecuados para un propósito específico. Use el juicio profesional para decisiones legales, médicas, financieras, de seguridad y operativas.'
						]
					},
					{
						title: 'Licencia de código abierto y disponibilidad',
						body: [
							'El código fuente de Anda Bot se proporciona bajo la licencia del repositorio. Estos términos no reemplazan la licencia de código abierto del código.',
							'El sitio web, la extensión, las versiones, la documentación y las integraciones pueden cambiar, pausarse o detenerse en cualquier momento. Las funciones se proporcionan según estén disponibles y sin un compromiso de nivel de servicio por separado.'
						]
					},
					{
						title: 'Sin garantía y limitación de responsabilidad',
						body: [
							'En la medida máxima permitida por la ley, Anda Bot se proporciona tal cual, sin garantías de ningún tipo.',
							'En la medida máxima permitida por la ley, los mantenedores del proyecto no son responsables de daños indirectos, incidentales, especiales, consecuentes o punitivos, ni de la pérdida de datos, ganancias, negocios o fondo de comercio.'
						]
					}
				],
				actions: [
					{ label: 'Leer política de privacidad', href: '/privacy' },
					{ label: 'Repositorio de GitHub', href: 'https://github.com/ldclabs/anda-bot' }
				]
			},
			support: {
				meta: {
					title: 'Soporte - Anda Bot',
					description:
						'Dónde obtener ayuda con la instalación de Anda Bot, la configuración de la extensión de Chrome, la conexión del token del navegador, los problemas del daemon local y los informes de errores.'
				},
				eyebrow: 'Soporte',
				title: 'Obtener ayuda con Anda Bot',
				intro:
					'Anda Bot es un agente local de código abierto. La forma más rápida de obtener soporte suele ser verificar la configuración, capturar el error exacto y abrir un problema en GitHub con suficientes detalles para reproducirlo.',
				updated: '15 de mayo de 2026',
				sections: [
					{
						title: 'Los mejores canales de soporte',
						items: [
							{
								title: 'Informes de errores',
								detail:
									'Abra un problema en GitHub para problemas reproducibles de la extensión, el daemon, la instalación, la herramienta del navegador o la compilación.'
							},
							{
								title: 'Preguntas e ideas',
								detail:
									'Use las discusiones de GitHub para preguntas de uso, flujos de trabajo, ideas de funciones y ayuda de la comunidad.'
							},
							{
								title: 'Documentación',
								detail:
									'Use la documentación y el README para comandos de instalación, configuración del proveedor de modelos, configuración del token de Chrome, Skills, canales, voz y contexto de resolución de problemas.'
							}
						]
					},
					{
						title: 'Antes de abrir un problema',
						items: [
							{
								title: 'Confirmar la configuración local',
								detail:
									'Ejecute anda desde una terminal, confirme que un proveedor de modelos esté configurado y asegúrese de que el daemon sea accesible antes de probar la extensión.'
							},
							{
								title: 'Regenerar el token del navegador',
								detail:
									'Ejecute anda browser token --days 365 y pegue tanto la Gateway URL como el Bearer token en la configuración del panel lateral.'
							},
							{
								title: 'Verificar permisos',
								detail:
									'Chrome puede bloquear la inyección de páginas, el acceso al micrófono, las URL de archivos o las acciones de extensión en páginas restringidas. Pruebe una página https normal al depurar.'
							}
						]
					},
					{
						title: 'Incluye esta información',
						body: [
							'Incluye su sistema operativo, versión de Anda Bot, versión de Chrome, versión de la extensión, método de instalación, el comando que ejecutó, el mensaje de error exacto y si el problema ocurre en una pestaña nueva del navegador.',
							'No pegue claves API, Bearer tokens, prompts privados, archivos confidenciales o capturas de pantalla sensibles en problemas públicos.'
						]
					},
					{
						title: 'Informes de seguridad y privacidad',
						body: [
							'Para problemas sensibles de seguridad, evite publicar secretos o detalles de exploits en un problema público. Abra primero un informe minimal o use las opciones de informe de seguridad del repositorio si están disponibles.',
							'Si un token puede haber estado expuesto, revóquelo eliminando la configuración de la extensión y generando un nuevo token de navegador desde la CLI local de Anda.'
						]
					}
				],
				actions: [
					{ label: 'Abrir problemas de GitHub', href: githubIssues },
					{ label: 'Abrir discusiones', href: githubDiscussions },
					{ label: 'Leer la documentación', href: docsUrl }
				]
			}
		}
	},
	fr: {
		common: {
			home: 'Accueil',
			docs: 'Docs',
			github: 'GitHub',
			privacy: 'Politique de confidentialité',
			terms: 'Conditions d’utilisation',
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
						'Comment l’extension Chrome et le site web d’Anda Bot gèrent la configuration locale, le contexte du navigateur, les prompts, les fonctionnalités vocales et les données des fournisseurs de modèles.'
				},
				eyebrow: 'Politique de confidentialité',
				title: 'Confidentialité, contrôle local et données du navigateur',
				intro:
					'Anda Bot est conçu comme un pont d’agent local-first. L’extension Chrome connecte votre navigateur au daemon Anda que vous exécutez, tandis que le site web explique et distribue le projet.',
				updated: '15 mai 2026',
				sections: [
					{
						title: 'Ce que couvre cette politique',
						body: [
							'Cette politique couvre le site web d’Anda Bot et l’extension Chrome d’Anda Bot. L’extension est un client de panneau latéral pour un daemon Anda local. Le daemon local, vos fournisseurs de modèles configurés et tous les services externes que vous connectez peuvent avoir leurs propres pratiques en matière de données.',
							'L’extension ne vend pas de données personnelles. Elle existe pour envoyer vos instructions et le contexte de navigateur sélectionné au runtime Anda local que vous configurez.'
						]
					},
					{
						title: 'Données stockées par l’extension',
						items: [
							{
								title: 'Paramètres de connexion',
								detail:
									'L’URL de la passerelle (Gateway URL) et le jeton porteur (Bearer token) que vous collez dans le panneau des paramètres sont stockés dans le stockage local de Chrome afin que l’extension puisse se reconnecter à votre daemon Anda local.'
							},
							{
								title: 'ID de session du navigateur',
								detail:
									'Un ID de session de navigateur stable est stocké localement afin qu’Anda puisse conserver un fil de discussion unique lorsque vous changez d’onglet.'
							},
							{
								title: 'État d’affichage de la conversation',
								detail:
									'Le panneau latéral peut afficher l’historique des conversations renvoyé par le daemon Anda local, mais les enregistrements de conversation sont gérés par le daemon plutôt que par un service d’extension distant.'
							}
						]
					},
					{
						title: 'Contexte du navigateur et autorisations',
						body: [
							'Une fois connectée, l’extension peut enregistrer l’ID de l’onglet actuel, son titre et son URL auprès de votre daemon Anda local. Si une tâche nécessite un travail dans le navigateur, le daemon peut demander à l’extension de lister les onglets, d’ouvrir ou de changer d’onglets, de naviguer, de capturer l’onglet visible ou d’exécuter des actions sur la page comme lire le contenu, cliquer, taper, faire défiler et appuyer sur des touches.',
							'Les autorisations TTS permettent à Anda de prononcer les réponses via Chrome lorsque vous choisissez la lecture vocale. La capture vocale ou la reconnaissance vocale ne démarre qu’à partir des commandes vocales destinées à l’utilisateur et peut dépendre des autorisations du microphone du navigateur pour la page active.'
						]
					},
					{
						title: 'Prompts, fichiers et fournisseurs de modèles',
						body: [
							'Vos prompts, pièces jointes sélectionnées, transcriptions générées, captures d’écran, texte de page et résultats d’outils peuvent être envoyés au daemon Anda local. Le daemon peut ensuite envoyer le contenu pertinent aux fournisseurs de modèles et aux services que vous configurez dans Anda.',
							'Les clés API et les paramètres du fournisseur sont contrôlés par votre configuration Anda locale. Examinez les conditions de confidentialité de chaque fournisseur avant d’envoyer des informations sensibles.'
						]
					},
					{
						title: 'Rétention et contrôle',
						items: [
							{
								title: 'Données de l’extension locale',
								detail:
									'Vous pouvez effacer l’URL de la passerelle, le jeton et les données de session via le stockage de l’extension Chrome ou en supprimant l’extension.'
							},
							{
								title: 'Données du runtime Anda',
								detail:
									'Anda stocke l’état du runtime, les journaux, les canaux, les fichiers de l’espace de travail et les données de mémoire dans le répertoire personnel local d’Anda, à moins que vous ne configuriez un autre emplacement.'
							},
							{
								title: 'Analyses du site web',
								detail:
									'Le code actuel du site web ne comprend pas de tracker d’analyse personnalisé. L’infrastructure d’hébergement peut toujours créer des journaux opérationnels standard.'
							}
						]
					},
					{
						title: 'Utilisation sensible',
						body: [
							'N’envoyez pas de secrets, de documents réglementés, de données commerciales confidentielles ou tout autre contenu sensible à moins de comprendre où votre daemon local, vos outils, vos fournisseurs de modèles et vos services connectés les traiteront.',
							'Anda Bot n’est pas destiné aux enfants de moins de l’âge requis par la loi applicable pour utiliser des services en ligne sans le consentement des parents.'
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
						'Conditions d’utilisation du site web d’Anda Bot, de l’extension Chrome, du pont de runtime local, des fonctionnalités d’automatisation du navigateur et des ressources de projet open-source.'
				},
				eyebrow: 'Conditions d’utilisation',
				title: 'Conditions pour utiliser Anda Bot',
				intro:
					'Ces conditions expliquent les responsabilités liées à l’utilisation du site web, de l’extension Chrome et du pont de runtime Anda local.',
				updated: '15 mai 2026',
				sections: [
					{
						title: 'Acceptation et portée',
						body: [
							'En utilisant le site web ou l’extension Chrome, vous acceptez ces conditions. Si vous ne les acceptez pas, n’utilisez pas le site web ni l’extension.',
							'Anda Bot est un logiciel open-source et un système d’agents local-first. Certaines fonctionnalités dépendent de votre installation locale, de vos fournisseurs de modèles, de vos navigateurs, de votre système d’exploitation et d’outils tiers.'
						]
					},
					{
						title: 'Votre configuration et vos comptes',
						items: [
							{
								title: 'Daemon local',
								detail:
									'Vous êtes responsable de l’installation, de la configuration, de la mise à jour et de la sécurisation du programme Anda Bot local ainsi que de tout répertoire personnel ou espace de travail qu’il utilise.'
							},
							{
								title: 'Clés API',
								detail:
									'Vous êtes responsable de vos comptes de fournisseurs de modèles, de vos clés API, de vos coûts d’utilisation, de vos limites de débit et des conditions du fournisseur.'
							},
							{
								title: 'Jeton d’extension',
								detail:
									'Gardez le Bearer token privé. Toute personne y ayant accès peut se connecter à votre passerelle Anda locale tant qu’elle est accessible.'
							}
						]
					},
					{
						title: 'Automatisation du navigateur',
						body: [
							'L’extension peut aider Anda à interagir avec les onglets et les pages du navigateur. Vous êtes responsable de l’examen des actions avant de les utiliser dans des comptes, des systèmes administratifs, des achats, des flux financiers, des services de production ou d’autres environnements sensibles.',
							'N’utilisez pas Anda Bot pour violer des sites web, des conditions d’utilisation, des contrôles d’accès, des lois ou les droits d’autrui.'
						]
					},
					{
						title: 'Contenu et résultats',
						body: [
							'Vous conservez la responsabilité des prompts, des fichiers, du contenu du navigateur et des autres éléments que vous fournissez. Vous êtes responsable de la vérification des résultats générés avant de vous y fier.',
							'Les résultats générés par l’IA peuvent être incomplets, incorrects, dangereux ou inadaptés à un usage spécifique. Utilisez votre jugement professionnel pour les décisions juridiques, médicales, financières, de sécurité et opérationnelles.'
						]
					},
					{
						title: 'Licence open-source et disponibilité',
						body: [
							'Le code source d’Anda Bot est fourni sous la licence présente dans le dépôt. Ces conditions ne remplacent pas la licence open-source du code.',
							'Le site web, l’extension, les versions, la documentation et les intégrations peuvent changer, être suspendus ou arrêtés à tout moment. Les fonctionnalités sont fournies en l’état et sans engagement de niveau de service (SLA) distinct.'
						]
					},
					{
						title: 'Exclusion de garantie et limitation de responsabilité',
						body: [
							'Dans la mesure maximale autorisée par la loi, Anda Bot est fourni en l’état, sans aucune garantie d’aucune sorte.',
							'Dans la mesure maximale autorisée par la loi, les mainteneurs du projet ne sont pas responsables des dommages indirects, accessoires, spéciaux, consécutifs ou punitifs, ni de la perte de données, de bénéfices, d’activités ou de clientèle.'
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
						'Où obtenir de l’aide pour l’installation d’Anda Bot, la configuration de l’extension Chrome, la connexion du jeton du navigateur, les problèmes du daemon local et les rapports de bugs.'
				},
				eyebrow: 'Support',
				title: 'Obtenir de l’aide avec Anda Bot',
				intro:
					'Vous devez vérifier la configuration locale, capturer l’erreur exacte et ouvrir un ticket GitHub avec suffisamment de détails pour reproduire l’erreur.',
				updated: '15 mai 2026',
				sections: [
					{
						title: 'Meilleurs canaux de support',
						items: [
							{
								title: 'Rapports de bugs',
								detail:
									'Ouvrez un ticket GitHub pour les problèmes reproductibles d’extension, de daemon, d’installation, d’outil de navigateur ou de compilation.'
							},
							{
								title: 'Questions et idées',
								detail:
									'Utilisez les discussions GitHub pour les questions d’utilisation, les flux de travail, les idées de fonctionnalités et l’aide de la communauté.'
							},
							{
								title: 'Documentation',
								detail:
									'Consultez la doc et le README pour les commandes d’installation, la configuration du fournisseur de modèles, la configuration du jeton Chrome, les Skills, les canaux, la voix et le contexte de dépannage.'
							}
						]
					},
					{
						title: 'Avant d’ouvrir un ticket',
						items: [
							{
								title: 'Confirmer la configuration locale',
								detail:
									'Lancez anda depuis un terminal, confirmez qu’un fournisseur de modèles est configuré et assurez-vous que le daemon est accessible avant de tester l’extension.'
							},
							{
								title: 'Régenerer le jeton du navigateur',
								detail:
									'Exécutez anda browser token --days 365 et collez l’URL de la passerelle et le jeton porteur dans les paramètres du panneau latéral.'
							},
							{
								title: 'Vérifier les autorisations',
								detail:
									'Chrome peut bloquer l’injection de page, l’accès au microphone, les URL de fichiers ou les actions de l’extension sur les pages restreintes. Essayez une page https normale lors du dépannage.'
							}
						]
					},
					{
						title: 'Inclure ces informations',
						body: [
							'Veuillez inclure votre système d’exploitation, la version d’Anda Bot, la version de Chrome, la version de l’extension, la méthode d’installation, la commande que vous avez exécutée, le message d’erreur exact et si le problème se produit sur un nouvel onglet du navigateur.',
							'Ne collez pas de clés API, de jetons porteurs, de prompts privés, de fichiers confidentiels ou de captures d’écran sensibles dans les tickets publics.'
						]
					},
					{
						title: 'Rapports de sécurité et de confidentialité',
						body: [
							'Pour les problèmes sensibles de sécurité, évitez de publier des secrets ou des détails d’exploitation dans un ticket public. Ouvrez d’abord un rapport minimal ou utilisez les options de rapport de sécurité du dépôt si elles sont disponibles.',
							'Si un jeton a pu être exposé, révoquez-le en supprimant les paramètres de l’extension et en générant un nouveau jeton de navigateur à partir de la CLI locale d’Anda.'
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
			privacy: 'Политика конфиденциальности',
			terms: 'Условия обслуживания',
			support: 'Поддержка',
			languageLabel: 'Язык',
			updatedLabel: 'Последнее обновление',
			navigationLabel: 'Навигация по сайту',
			moreLinks: 'Дополнительная информация'
		},
		pages: {
			privacy: {
				meta: {
					title: 'Политика конфиденциальности - Anda Bot',
					description:
						'Как расширение Anda Bot для Chrome и веб-сайт обрабатывают локальные настройки, контекст браузера, запросы, голосовые функции и данные поставщиков моделей.'
				},
				eyebrow: 'Политика конфиденциальности',
				title: 'Конфиденциальность, локальный контроль и данные браузера',
				intro:
					'Anda Bot разработан как локальный мост для агента. Расширение Chrome соединяет ваш браузер с запущенным вами демоном Anda, а веб-сайт объясняет устройство проекта и распространяет его.',
				updated: '15 мая 2026 г.',
				sections: [
					{
						title: 'Что охватывает эта политика',
						body: [
							'Эта политика распространяется на веб-сайт Anda Bot и расширение Anda Bot для Chrome. Расширение является клиентом боковой панели для локального демона Anda. Локальный демон, настроенные вами поставщики моделей и любые внешние службы, к которым вы подключаетесь, могут иметь собственные правила работы с данными.',
							'Расширение не продает личные данные. Оно существует для отправки ваших инструкций и выбранного контекста браузера в настроенную вами локальную среду выполнения Anda.'
						]
					},
					{
						title: 'Данные, хранимые расширением',
						items: [
							{
								title: 'Настройки подключения',
								detail:
									'URL шлюза (Gateway URL) и токен авторизации (Bearer token), которые вы вставляете в панель настроек, сохраняются в локальном хранилище Chrome, чтобы расширение могло повторно подключаться к вашему локальному демону Anda.'
							},
							{
								title: 'Идентификатор сессии браузера',
								detail:
									'Стабильный идентификатор сессии браузера сохраняется локально, чтобы Anda могла поддерживать одну цепочку диалога в браузере при переключении вкладок.'
							},
							{
								title: 'Состояние отображения диалога',
								detail:
									'Боковая панель может отображать историю диалогов, возвращаемую локальным демоном Anda, но записи разговоров управляются демоном, а не удаленной службой расширения.'
							}
						]
					},
					{
						title: 'Контекст браузера и разрешения',
						body: [
							'При подключении расширение может зарегистрировать идентификатор текущей вкладки, ее заголовок и URL в локальном демоне Anda. Если задача требует работы в браузере, демон может попросить расширение составить список вкладок, открыть или переключить вкладки, выполнить переход, сделать снимок видимой вкладки или выполнить действия на странице, такие как чтение содержимого, нажатие кнопок, ввод текста, прокрутка и нажатие клавиш.',
							'Разрешения TTS позволяют Anda озвучивать ответы через Chrome, когда вы выбираете голосовое воспроизведение. Запись голоса или распознавание речи запускаются только с помощью элементов управления голосом со стороны пользователя и могут зависеть от разрешений на доступ к микрофону для активной страницы.'
						]
					},
					{
						title: 'Запросы, файлы и поставщики моделей',
						body: [
							'Ваши запросы, выбранные вложения, сгенерированные стенограммы, снимки экрана, текст страниц и результаты работы инструментов могут отправляться локальному демону Anda. Затем демон может отправлять соответствующий контент поставщикам моделей и службам, настроенным в Anda.',
							'Ключи API и настройки поставщиков управляются вашей локальной конфигурацией Anda. Ознакомьтесь с условиями конфиденциальности каждого поставщика перед отправкой конфиденциальной информации.'
						]
					},
					{
						title: 'Хранение и контроль',
						items: [
							{
								title: 'Локальные данные расширения',
								detail:
									'Вы можете очистить URL-адрес шлюза, токен и данные сеанса через хранилище расширений Chrome или удалив расширение.'
							},
							{
								title: 'Данные выполнения Anda',
								detail:
									'Anda сохраняет состояние выполнения, журналы, каналы, файлы рабочей области и данные памяти в локальном домашнем каталоге Anda, если вы не настроили другое расположение.'
							},
							{
								title: 'Аналитика веб-сайта',
								detail:
									'Текущий код веб-сайта не содержит встроенного счетчика аналитики. Хостинг-провайдер по-прежнему может создавать стандартные операционные журналы.'
							}
						]
					},
					{
						title: 'Чувствительная информация',
						body: [
							'Не отправляйте пароли, регулируемые записи, конфиденциальные бизнес-данные или другие важные сведения, если вы не понимаете, где ваш локальный демон, инструменты, поставщики моделей и подключенные службы будут их обрабатывать.',
							'Anda Bot не предназначен для детей младше возраста, установленного применимым законодательством для использования онлайн-услуг без согласия родителей.'
						]
					}
				],
				actions: [
					{ label: 'Открыть поддержку', href: '/support' },
					{ label: 'GitHub issues', href: githubIssues }
				]
			},
			terms: {
				meta: {
					title: 'Условия обслуживания - Anda Bot',
					description:
						'Условия использования веб-сайта Anda Bot, расширения Chrome, локального моста выполнения, функций автоматизации браузера и ресурсов проекта с открытым исходным кодом.'
				},
				eyebrow: 'Условия обслуживания',
				title: 'Правила использования Anda Bot',
				intro:
					'Эти условия определяют ответственность, связанную с использованием веб-сайта, расширения Chrome и локального моста выполнения Anda.',
				updated: '15 мая 2026 г.',
				sections: [
					{
						title: 'Согласие и область действия',
						body: [
							'Используя веб-сайт или расширение Chrome, вы соглашаетесь с настоящими условиями. Если вы не согласны, не используйте их.',
							'Anda Bot — это программное обеспечение с открытым исходным кодом и локальная агентная система. Некоторые функции зависят от вашей локальной установки, поставщиков моделей, браузеров, операционной системы и сторонних инструментов.'
						]
					},
					{
						title: 'Ваша конфигурация и учетные записи',
						items: [
							{
								title: 'Локальный демон',
								detail:
									'Вы несете ответственность за установку, настройку, обновление и безопасность локального приложения Anda Bot, а также любого домашнего каталога или рабочей области, которые оно использует.'
							},
							{
								title: 'Ключи API',
								detail:
									'Вы несете ответственность за свои учетные записи поставщиков моделей, ключи API, расходы на использование, лимиты запросов и условия обслуживания поставщиков.'
							},
							{
								title: 'Токен расширения',
								detail:
									'Храните Bearer-токен в секрете. Любой, кто имеет к нему доступ, может подключиться к вашему локальному шлюзу Anda, пока он доступен по сети.'
							}
						]
					},
					{
						title: 'Автоматизация браузера',
						body: [
							'Расширение помогает Anda взаимодействовать со вкладками и страницами браузера. Вы несете ответственность за проверку действий перед их выполнением в учетных записях, административных системах, при покупках, финансовых операциях, на рабочих серверах или в других конфиденциальных средах.',
							'Не используйте Anda Bot для взлома сайтов, нарушения условий обслуживания, систем контроля доступа, законов или прав других лиц.'
						]
					},
					{
						title: 'Контент и результаты',
						body: [
							'Вы несете полную ответственность за предоставляемые запросы, файлы, содержимое страниц браузера и другие материалы. Вы обязаны проверять результаты генерации ИИ, прежде чем полагаться на них.',
							'Результаты, созданные ИИ, могут быть неполными, неточными, небезопасными или неподходящими для конкретной цели. Руководствуйтесь здрамым смыслом и мнением специалистов при принятии юридических, медицинских, финансовых решений, а также в вопросах безопасности и эксплуатации.'
						]
					},
					{
						title: 'Лицензия с открытым исходным кодом и доступность',
						body: [
							'Исходный код Anda Bot предоставляется на условиях лицензии, указанной в репозитории. Настоящие условия не заменяют лицензию на исходный код.',
							'Сайт, расширение, выпуски программного обеспечения, документация и интеграции могут быть изменены, приостановлены или прекращены в любое время. Функции предоставляются по мере доступности без отдельного соглашения об уровне услуг (SLA).'
						]
					},
					{
						title: 'Отказ от гарантий и ограничение ответственности',
						body: [
							'В максимально разрешенной законом степени Anda Bot предоставляется «как есть», без каких-либо гарантий.',
							'В максимально разрешенной законом степени разработчики проекта не несут ответственности за косвенные, случайные, специальные или штрафные убытки, а также за потерю данных, прибыли, бизнеса или деловой репутации.'
						]
					}
				],
				actions: [
					{ label: 'Политика конфиденциальности', href: '/privacy' },
					{ label: 'GitHub', href: 'https://github.com/ldclabs/anda-bot' }
				]
			},
			support: {
				meta: {
					title: 'Поддержка - Anda Bot',
					description:
						'Где получить помощь по установке Anda Bot, настойке расширения Chrome, подключению токена браузера, локальному демону и сообщениям об ошибках.'
				},
				eyebrow: 'Поддержка',
				title: 'Получить помощь по Anda Bot',
				intro:
					'Самый быстрый способ получить помощь — проверить настройки, зафиксировать точный текст ошибки и создать тему на GitHub с подробным описанием воспроизведения.',
				updated: '15 мая 2026 г.',
				sections: [
					{
						title: 'Лучшие каналы поддержки',
						items: [
							{
								title: 'Отчеты об ошибках',
								detail:
									'Откройте issue на GitHub при наличии воспроизводимых проблем в расширении, демоне, установщике, браузерных инструментах или при сборке.'
							},
							{
								title: 'Вопросы и идеи',
								detail:
									'Используйте обсуждения (Discussions) на GitHub для вопросов по использованию, сценариев автоматизации, идей и помощи сообщества.'
							},
							{
								title: 'Документация',
								detail:
									'Обращайтесь к документации и файлу README для получения команд установки, настройки поставщиков моделей, токена Chrome, Skills, каналов, голоса и решения проблем.'
							}
						]
					},
					{
						title: 'Перед созданием темы',
						items: [
							{
								title: 'Проверка локальной установки',
								detail:
									'Запустите anda в терминале, убедитесь в наличии настроенного провайдера моделей и доступности демона перед тестированием расширения.'
							},
							{
								title: 'Обновление токена браузера',
								detail:
									'Выполните команду anda browser token --days 365 и скопируйте Gateway URL и Bearer token в настройки боковой панели.'
							},
							{
								title: 'Проверка прав доступа',
								detail:
									'Chrome может блокировать внедрение скриптов, доступ к микрофону, протокол file:// или действия расширения на защищенных страницах. Попробуйте обычную страницу https при отладке.'
							}
						]
					},
					{
						title: 'Предоставляемая информация',
						body: [
							'Пожалуйста, укажите вашу операционную систему, версию Anda Bot, версию Chrome, версию расширения, способ установки, выполнявшуюся команду, точное сообщение об ошибке и воспроизводится ли проблема на новой пустой вкладке.',
							'Никогда не публикуйте ключи API, Bearer-токены, личные запросы, конфиденциальные файлы или снимки экрана с личными данными в публичных темах.'
						]
					},
					{
						title: 'Отчеты о безопасности и конфиденциальности',
						body: [
							'При возникновении проблем, связанных с безопасностью, не публикуйте ключи или детали эксплойтов открыто. Сначала создайте минимальное описание проблемы или воспользуйтесь специальной формой сообщения о проблемах безопасности в репозитории.',
							'Если токен мог быть скомпрометирован, немедленно удалите его из настроек расширения и сгенерируйте новый с помощью локального CLI Anda.'
						]
					}
				],
				actions: [
					{ label: 'Открыть GitHub issues', href: githubIssues },
					{ label: 'Открыть обсуждения', href: githubDiscussions },
					{ label: 'Читать документацию', href: docsUrl }
				]
			}
		}
	},
	ar: {
		common: {
			home: 'الرئيسية',
			docs: 'الوثائق',
			github: 'GitHub',
			privacy: 'سياسة الخصوصية',
			terms: 'شروط الخدمة',
			support: 'الدعم الفني',
			languageLabel: 'اللغة',
			updatedLabel: 'آخر تحديث في',
			navigationLabel: 'التنقل في الموقع',
			moreLinks: 'مزيد من المعلومات'
		},
		pages: {
			privacy: {
				meta: {
					title: 'سياسة الخصوصية - Anda Bot',
					description:
						'كيفية تعامل موقع Anda Bot وإضافة Chrome مع الإعدادات المحلية وسياق المتصفح والمطالبات وميزات الصوت وبيانات مزود النموذج.'
				},
				eyebrow: 'سياسة الخصوصية',
				title: 'الخصوصية والتحكم المحلي وبيانات المتصفح',
				intro:
					'تم تصميم Anda Bot كجسر للوكيل المحلي أولاً. تقوم إضافة Chrome بربط متصفحك بـ Anda daemon الذي تقوم بتشغيله، بينما يقوم الموقع الإلكتروني بشرح وتوزيع المشروع.',
				updated: '15 مايو 2026',
				sections: [
					{
						title: 'ما تغطيه هذه السياسة',
						body: [
							'تغطي هذه السياسة موقع Anda Bot وإضافة Anda Bot لمتصفح Chrome. الإضافة عبارة عن عميل لوحة جانبية لـ Anda daemon المحلي. قد يكون للـ daemon المحلي ومزودي النماذج الذين قمت بتكوينهم وأي خدمات خارجية تتصل بها ممارسات البيانات الخاصة بهم.',
							'لا تقوم الإضافة ببيع البيانات الشخصية. تم إنشاؤها لإرسال تعليماتك وسياق المتصفح المحدد إلى بيئة تشغيل Anda المحلية التي تقوم بتكوينها.'
						]
					},
					{
						title: 'البيانات المخزنة بواسطة الإضافة',
						items: [
							{
								title: 'إعدادات الاتصال',
								detail:
									'يتم تخزين Gateway URL و Bearer token اللذين تقوم بلصقهما في لوحة الإعدادات في التخزين المحلي لـ Chrome حتى تتمكن الإضافة من إعادة الاتصال بـ Anda daemon المحلي.'
							},
							{
								title: 'معرف جلسة المتصفح',
								detail:
									'يتم تخزين معرف جلسة متصفح مستقر محليًا حتى يتمكن Anda من الحفاظ على سلسلة محادثة متصفح واحدة أثناء تبديل التبويبات.'
							},
							{
								title: 'حالة عرض المحادثة',
								detail:
									'قد تعرض اللوحة الجانبية سجل المحادثات المسترجع من Anda daemon المحلي، ولكن يتم إدارة سجلات المحادثة بواسطة الـ daemon بدلاً من خدمة الإضافة البعيدة.'
							}
						]
					},
					{
						title: 'سياق المتصفح والأذونات',
						body: [
							'عند الاتصال، يمكن للإضافة تسجيل معرف التبويب الحالي وعنوانه ورابطه (URL) مع Anda daemon المحلي. إذا كانت المهمة تتطلب عملاً في المتصفح، فقد يطلب الـ daemon من الإضافة إدراج التبويبات، أو فتح أو تبديل التبويبات، أو التنقل، أو التقاط التبويب المرئي، أو تشغيل إجراءات الصفحة مثل قراءة المحتوى والنقر والكتابة والتمرير والضغط على المفاتيح.',
							'تتيح أذونات تحويل النص إلى كلام (TTS) لـ Anda نطق الاستجابات عبر Chrome عندما تختار تشغيل الصوت. لا يبدأ التقاط الصوت أو التعرف على الكلام إلا من خلال عناصر التحكم الصوتية التي تواجه المستخدم وقد يعتمد على أذونات ميكروفون المتصفح للصفحة النشطة.'
						]
					},
					{
						title: 'المطالبات والملفات ومزودو النماذج',
						body: [
							'يمكن إرسال مطالباتك والمرفقات المحددة والنصوص البرمجية التي تم إنشاؤها ولقطات الشاشة ونص الصفحة ونتائج الأدوات إلى Anda daemon المحلي. قد يقوم الـ daemon بعد ذلك بإرسال المحتوى ذي الصلة إلى مزودي النماذج والخدمات التي تقوم بتكوينها في Anda.',
							'يتم التحكم في مفاتيح API وإعدادات المزود بواسطة تكوين Anda المحلي الخاص بك. راجع شروط الخصوصية لكل مزود قبل إرسال معلومات حساسة.'
						]
					},
					{
						title: 'الاحتفاظ والتحكم',
						items: [
							{
								title: 'بيانات الإضافة المحلية',
								detail:
									'يمكنك مسح Gateway URL والتوكن وبيانات الجلسة من خلال تخزين إضافة Chrome أو عن طريق إزالة الإضافة.'
							},
							{
								title: 'بيانات تشغيل Anda',
								detail:
									'يقوم Anda بتخزين حالة التشغيل والسجلات والقنوات وملفات مساحة العمل وبيانات الذاكرة في دليل Anda الرئيسي المحلي ما لم تقم بتكوين موقع مختلف.'
							},
							{
								title: 'تحليلات الموقع الإلكتروني',
								detail:
									'لا يتضمن كود الموقع الحالي متتبع تحليلات مخصصًا. قد تستمر البنية التحتية للاستضافة في إنشاء سجلات تشغيل قياسية.'
							}
						]
					},
					{
						title: 'الاستخدام الحساس',
						body: [
							'لا ترسل أسرارًا أو سجلات خاضعة للتنظيم أو بيانات عمل سرية أو محتويات حساسة أخرى ما لم تكن تفهم أين سيقوم الـ daemon المحلي والأدوات ومزودو النماذج والخدمات المتصلة بمعالجتها.',
							'Anda Bot ليس مخصصًا للأطفال دون السن المطلوبة بموجب القانون المعمول به لاستخدام الخدمات عبر الإنترنت دون موافقة الوالدين.'
						]
					}
				],
				actions: [
					{ label: 'فتح الدعم الفني', href: '/support' },
					{ label: 'مشكلات GitHub', href: githubIssues }
				]
			},
			terms: {
				meta: {
					title: 'شروط الخدمة - Anda Bot',
					description:
						'شروط استخدام موقع Anda Bot وإضافة Chrome وجسر التشغيل المحلي وميزات أتمتة المتصفح وموارد مشروع مفتوح المصدر.'
				},
				eyebrow: 'شروط الخدمة',
				title: 'شروط استخدام Anda Bot',
				intro:
					'توضح هذه الشروط المسؤوليات التي تصاحب استخدام الموقع الإلكتروني وإضافة Chrome وجسر تشغيل Anda المحلي.',
				updated: '15 مايو 2026',
				sections: [
					{
						title: 'القبول والنطاق',
						body: [
							'باستخدام الموقع الإلكتروني أو إضافة Chrome، فإنك توافق على هذه الشروط. إذا كنت لا توافق، فلا تستخدم الموقع أو الإضافة.',
							'Anda Bot هو برنامج مفتوح المصدر ونظام وكيل محلي أولاً. تعتمد بعض الوظائف على التثبيت المحلي ومزودي النماذج والمتصفحات ونظام التشغيل وأدوات الطرف الثالث.'
						]
					},
					{
						title: 'إعدادك وحساباتك',
						items: [
							{
								title: 'الـ daemon المحلي',
								detail:
									'أنت مسؤول عن تثبيت برنامج Anda Bot المحلي وتكوينه وتحديثه وتأمينه وأي دليل رئيسي أو مساحة عمل يستخدمها.'
							},
							{
								title: 'مفاتيح API',
								detail:
									'أنت مسؤول عن حسابات مزودي النماذج الخاصة بك ومفاتيح API وتكاليف الاستخدام وقيود المعدل وشروط المزود.'
							},
							{
								title: 'توكن الإضافة',
								detail:
									'حافظ على سرية Bearer token. قد يتمكن أي شخص لديه إمكانية الوصول إليه من الاتصال ببوابة Anda المحلية الخاصة بك طالما كان من الممكن الوصول إليها.'
							}
						]
					},
					{
						title: 'أتمتة المتصفح',
						body: [
							'يمكن للإضافة مساعدة Anda في التفاعل مع تبويبات وصفحات المتصفح. أنت مسؤول عن مراجعة الإجراءات قبل استخدامها في الحسابات أو الأنظمة الإدارية أو المشتريات أو سير العمل المالي أو خدمات الإنتاج أو البيئات الحساسة الأخرى.',
							'لا تستخدم Anda Bot لانتهاك المواقع الإلكترونية أو شروط الخدمة أو ضوابط الوصول أو القوانين أو حقوق الآخرين.'
						]
					},
					{
						title: 'المحتوى والمخرجات',
						body: [
							'أنت تحتفظ بالمسؤولية عن المطالبات والملفات ومحتوى المتصفح والمواد الأخرى التي تقدمها. أنت مسؤول عن التحقق من المخرجات الناتجة قبل الاعتماد عليها.',
							'يمكن أن تكون المخرجات الناتجة عن الذكاء الاصطناعي غير كاملة أو غير صحيحة أو غير آمنة أو غير مناسبة لغرض معين. استخدم الحكم المهني للقرارات القانونية والطبية والمالية والأمنية والتشغيلية.'
						]
					},
					{
						title: 'ترخيص مفتوح المصدر وتوافر الخدمة',
						body: [
							'يتم توفير الكود المصدري لـ Anda Bot بموجب الترخيص الموجود في المستودع. لا تحل هذه الشروط محل الترخيص مفتوح المصدر للكود.',
							'قد يتغير الموقع الإلكتروني والإضافة والإصدارات والوثائق والتكاملات أو تتوقف مؤقتًا أو بشكل دائم في أي وقت. يتم تقديم الميزات كما هي متاحة ودون التزام منفصل بمستوى الخدمة (SLA).'
						]
					},
					{
						title: 'إخلاء المسؤولية وتحديد المسؤولية',
						body: [
							'إلى أقصى حد يسمح به القانون، يتم تقديم Anda Bot كما هو، دون أي ضمانات من أي نوع.',
							'إلى أقصى حد يسمح به القانون، لا يتحمل مشرفو المشروع المسؤولية عن أي أضرار غير مباشرة أو عرضية أو خاصة أو تبعية أو تأديبية، أو عن فقدان البيانات أو الأرباح أو الأعمال أو السمعة التجارية.'
						]
					}
				],
				actions: [
					{ label: 'قراءة سياسة الخصوصية', href: '/privacy' },
					{ label: 'مستودع GitHub', href: 'https://github.com/ldclabs/anda-bot' }
				]
			},
			support: {
				meta: {
					title: 'الدعم الفني - Anda Bot',
					description:
						'أين يمكن الحصول على المساعدة بشأن تثبيت Anda Bot وإعداد إضافة Chrome واتصال توكن المتصفح ومشكلات الـ daemon المحلي وتقارير الأخطاء.'
				},
				eyebrow: 'الدعم الفني',
				title: 'الحصول على المساعدة بشأن Anda Bot',
				intro:
					'Anda Bot هو وكيل محلي مفتوح المصدر. عادةً ما يكون أسرع مسار للحصول على الدعم هو التحقق من الإعداد، والتقاط الخطأ الدقيق، وفتح مشكلة على GitHub مع تفاصيل كافية لإعادة إنتاجها.',
				updated: '15 مايو 2026',
				sections: [
					{
						title: 'أفضل قنوات الدعم',
						items: [
							{
								title: 'تقارير الأخطاء',
								detail:
									'افتح مشكلة (Issue) على GitHub لمشكلات الإضافة أو الـ daemon أو التثبيت أو أداة المتصفح أو البناء القابلة لإعادة الإنتاج.'
							},
							{
								title: 'الأسئلة والأفكار',
								detail:
									'استخدم مناقشات GitHub لأسئلة الاستخدام وسير العمل وأفكار الميزات ومساعدة المجتمع.'
							},
							{
								title: 'الوثائق',
								detail:
									'استخدم الوثائق وملف README لأوامر التثبيت وتكوين مزود النموذج وإعداد توكن Chrome والمهارات والقنوات والصوت وسياق استكشاف الأخطاء وإصلاحها.'
							}
						]
					},
					{
						title: 'قبل فتح مشكلة',
						items: [
							{
								title: 'تأكيد الإعداد المحلي',
								detail:
									'قم بتشغيل anda من محطة الطرفية، وتأكد من تكوين مزود النموذج، وتأكد من إمكانية الوصول إلى الـ daemon قبل اختبار الإضافة.'
							},
							{
								title: 'إعادة إنشاء توكن المتصفح',
								detail:
									'قم بتشغيل anda browser token --days 365 والصق كلاً من Gateway URL و Bearer token في إعدادات اللوحة الجانبية.'
							},
							{
								title: 'التحقق من الأذونات',
								detail:
									'قد يحظر Chrome حقن الصفحة أو الوصول إلى الميكروفون أو روابط الملفات أو إجراءات الإضافة على الصفحات المقيدة. جرب صفحة https عادية عند استكشاف الأخطاء وإصلاحها.'
							}
						]
					},
					{
						title: 'تضمين هذه المعلومات',
						body: [
							'يرجى تضمين نظام التشغيل وإصدار Anda Bot وإصدار Chrome وإصدار الإضافة وطريقة التثبيت والأمر الذي قمت بتشغيله ورسالة الخطأ الدقيقة وما إذا كانت المشكلة تحدث في تبويب متصفح جديد.',
							'لا تقم بلصق مفاتيح API أو Bearer tokens أو المطالبات الخاصة أو الملفات السرية أو لقطات الشاشة الحساسة في المشكلات العامة.'
						]
					},
					{
						title: 'تقارير الأمن والخصوصية',
						body: [
							'بالنسبة للمشكلات الحساسة المتعلقة بالأمان، تجنب نشر الأسرار أو تفاصيل الثغرات في مشكلة عامة. افتح تقريرًا بسيطًا أولاً أو استخدم خيارات الإبلاغ عن الأمان في المستودع إذا كانت متاحة.',
							'إذا كان هناك احتمال لتعرض التوكن للكشف، فقم بإلغائه عن طريق إزالة إعدادات الإضافة وإنشاء توكن متصفح جديد من واجهة أوامر Anda المحلية.'
						]
					}
				],
				actions: [
					{ label: 'فتح مشكلات GitHub', href: githubIssues },
					{ label: 'فتح المناقشات', href: githubDiscussions },
					{ label: 'قراءة الوثائق', href: docsUrl }
				]
			}
		}
	}
};
