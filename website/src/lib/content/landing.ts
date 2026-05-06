export type Locale = 'ar' | 'zh' | 'en' | 'fr' | 'ru' | 'es';
export type TextDirection = 'ltr' | 'rtl';
export type OsKey = 'macos' | 'windows' | 'linux';

type InstallOptionCopy = {
	label: string;
	commandLabel: string;
	note: string;
	fallbackLabel?: string;
};

type ReasoningCardCopy = {
	label: string;
	title: string;
	detail: string;
};

type WorkSurfaceCopy = {
	label: string;
	detail: string;
};

type WorkCardCopy = {
	title: string;
	detail: string;
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
		reasoning: string;
		memory: string;
		surfaces: string;
	};
	language: {
		label: string;
	};
	hero: {
		badge: string;
		eyebrow: string;
		title: string;
		body: string;
		installFor: string;
		seeReasoning: string;
		proofOs: string;
		proofOsText: string;
		proofReasoning: string;
		proofReasoningText: string;
		proofMemory: string;
		proofMemoryText: string;
	};
	install: {
		eyebrow: string;
		title: string;
		detected: string;
		chooseOs: string;
		tabAria: string;
		copy: string;
		copied: string;
		copyFailed: string;
		copyAria: string;
		commandAria: string;
		alternative: string;
		steps: [string, string, string];
		requiresPrefix: string;
		requiresSuffix: string;
		options: Record<OsKey, InstallOptionCopy>;
	};
	reasoning: {
		badge: string;
		title: string;
		body: string;
		panelTitle: string;
		panelStatus: string;
		phases: [string, string, string];
		signals: Array<{ label: string; value: string; level: number }>;
		events: Array<{ time: string; phase: string; detail: string }>;
		cards: ReasoningCardCopy[];
	};
	memory: {
		badge: string;
		title: string;
		body: string;
		formationTitle: string;
		formationBody: string;
		recallTitle: string;
		recallBody: string;
		maintenanceTitle: string;
		maintenanceBody: string;
	};
	work: {
		contextRoutes: string;
		memoryRoute: string;
		badge: string;
		title: string;
		body: string;
		surfaces: WorkSurfaceCopy[];
		cards: WorkCardCopy[];
	};
	start: {
		badge: string;
		title: string;
		bodyPrefix: string;
		bodySuffix: string;
		quickStart: string;
		meetHippocampus: string;
		terminalLabel: string;
		sourceComment: string;
		goalComment: string;
		localRuntime: string;
		durableThread: string;
		inspectableBrain: string;
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

export const landingCopy: Record<Locale, LandingCopy> = {
	en: {
		meta: {
			title: 'Install Anda Bot - Graph-memory Rust agent',
			description:
				'Install Anda Bot, an open-source Rust agent with graph long-term memory, long-horizon reasoning, external tools, subagents, and IM integrations.',
			ogTitle: 'Install Anda Bot - Graph-memory Rust agent',
			ogDescription:
				'Run Anda locally with Anda Hippocampus memory, long-running goals, Claude Code and Codex tool use, subagents, and terminal or IM workflows.'
		},
		nav: {
			install: 'Install',
			reasoning: 'Reasoning',
			memory: 'Memory',
			surfaces: 'Surfaces'
		},
		language: { label: 'Language' },
		hero: {
			badge: 'Open-source Rust terminal agent',
			eyebrow: 'graph memory · subagents · external tools',
			title: 'Install Anda. Give it work that needs memory, tools, and time.',
			body: 'I am the local agent that keeps learning while we work. Hippocampus turns experience into graph memory, long-horizon goals keep moving across context boundaries, and subagents can coordinate external tools such as Claude Code and Codex.',
			installFor: 'Install for {os}',
			seeReasoning: 'See core loop',
			proofOs: 'Rust',
			proofOsText: 'open source',
			proofReasoning: 'hours+',
			proofReasoningText: 'goal loops',
			proofMemory: 'graph',
			proofMemoryText: 'memory'
		},
		install: {
			eyebrow: 'Install latest release',
			title: 'Run Anda locally',
			detected: 'detected {os}',
			chooseOs: 'choose OS',
			tabAria: 'Install method by operating system',
			copy: 'Copy',
			copied: 'Copied',
			copyFailed: 'Copy failed',
			copyAria: 'Copy install command',
			commandAria: 'Copy the install command',
			alternative: 'Alternative: {method}',
			steps: ['Install release', 'Add or export key', 'Run'],
			requiresPrefix: 'Requires at least one model provider API key in config or env. Anda creates',
			requiresSuffix: 'on first launch.',
			options: {
				macos: {
					label: 'macOS',
					commandLabel: 'Shell script',
					note: 'The install script fetches the latest release and curated skills for macOS.',
					fallbackLabel: 'Homebrew'
				},
				windows: {
					label: 'Windows',
					commandLabel: 'PowerShell',
					note: 'Run this in PowerShell, then open a new terminal and start Anda.'
				},
				linux: {
					label: 'Linux',
					commandLabel: 'Shell script',
					note: 'The install script fetches the latest release for your local runtime.'
				}
			}
		},
		reasoning: {
			badge: 'Long-horizon reasoning',
			title: 'A goal can keep moving after the chat window would normally stop.',
			body: 'Anda can keep a session alive across linked conversations. It compacts the current state, preserves the objective, asks subagents to inspect or continue focused work, and calls the tools needed to reach a verified result.',
			panelTitle: 'Anda session loop',
			panelStatus: 'goal://active',
			phases: ['reason & execute', 'compact & handoff', 'evaluate & continue'],
			signals: [
				{ label: 'objective', value: 'active', level: 92 },
				{ label: 'subagents', value: 'coordinating', level: 84 },
				{ label: 'tools', value: 'Claude Code / Codex', level: 76 },
				{ label: 'memory', value: 'forming', level: 68 }
			],
			events: [
				{ time: '00:01', phase: 'goal', detail: 'objective accepted and session opened' },
				{ time: '47:18', phase: 'tools', detail: 'external coding tools and files inspected' },
				{ time: 'hours+', phase: 'audit', detail: 'subagents continue until evidence is complete' }
			],
			cards: [
				{
					label: 'Long goals',
					title: 'Keeps working after one answer is not enough',
					detail:
						'Give Anda an objective and it can inspect progress, compact context, continue in a linked conversation, and keep going until the outcome is real.'
				},
				{
					label: 'Subagents',
					title: 'Delegates work without losing the main thread',
					detail:
						'Specialized subagents can research, implement, review, or supervise while the main session keeps the plan, memory, and final objective intact.'
				},
				{
					label: 'Tool use',
					title: 'Works with the tools already on your machine',
					detail:
						'Anda can call shell and file tools, load skills, and coordinate external coding tools such as Claude Code and Codex when the task needs them.'
				}
			]
		},
		memory: {
			badge: 'Anda Hippocampus',
			title: 'Memory is a graph that keeps learning the useful parts.',
			body: 'Hippocampus turns useful conversations into a living Cognitive Nexus: people, projects, decisions, preferences, events, timelines, and the relationships between them.',
			formationTitle: 'Formation',
			formationBody:
				'I distill successful turns, files, channel context, and tool results into memories with source, time, and meaning.',
			recallTitle: 'Recall',
			recallBody:
				'When a goal needs history, I retrieve relationships and timelines that explain the present task instead of matching nearest text alone.',
			maintenanceTitle: 'Maintenance',
			maintenanceBody:
				'I consolidate, merge, decay, and preserve timelines so durable knowledge stays reachable while old noise fades.'
		},
		work: {
			contextRoutes: 'context routes',
			memoryRoute: 'anda://memory',
			badge: 'Where I work',
			title: 'Bring long memory to the terminal, tools, and team channels.',
			body: 'I run as an open-source Rust terminal agent, join IM surfaces such as WeChat, Feishu/Lark, Telegram, Discord, and IRC, and keep the same memory thread moving through tools and subagents.',
			surfaces: [
				{
					label: 'terminal',
					detail:
						'A local Rust runtime keeps commands, files, and agent work close to your machine.'
				},
				{
					label: 'external tools',
					detail:
						'Claude Code, Codex, shell tools, skills, and files can all become part of the loop.'
				},
				{
					label: 'subagents',
					detail: 'Focused workers can research, implement, audit, and continue complex tasks.'
				},
				{
					label: 'IM channels',
					detail:
						'WeChat, Feishu/Lark, Telegram, Discord, and IRC can share the same memory thread.'
				}
			],
			cards: [
				{
					title: 'Rust terminal runtime',
					detail:
						'Open-source, local-first, and built for commands, files, experiments, and multi-step automation.'
				},
				{
					title: 'WeChat, Feishu, Telegram',
					detail: 'Work conversations from IM channels can feed the same durable context.'
				},
				{
					title: 'Claude Code and Codex',
					detail:
						'Use external coding assistants as tools while Anda keeps the objective and memory.'
				},
				{
					title: 'Powerful subagents',
					detail:
						'Delegate specialized work and supervision without scattering the project context.'
				}
			]
		},
		start: {
			badge: 'After install',
			title: 'Run Anda from any terminal with a model key.',
			bodyPrefix: 'Use an environment key for the first run, or save provider keys in',
			bodySuffix: 'for future sessions.',
			quickStart: 'Quick start',
			meetHippocampus: 'Meet Hippocampus',
			terminalLabel: 'run command',
			sourceComment: 'Start now with an environment key:',
			goalComment: 'Or save api_key in config.yaml, then run:',
			localRuntime: 'Rust runtime',
			durableThread: 'subagents',
			inspectableBrain: 'graph brain'
		}
	},
	zh: {
		meta: {
			title: '安装 Anda Bot - 知识图谱记忆智能体',
			description:
				'安装 Anda Bot，一个具备知识图谱长期记忆、长程推理、外部工具调用、子智能体 和 IM 接入能力的开源 Rust Agent。',
			ogTitle: '安装 Anda Bot - 知识图谱记忆智能体',
			ogDescription:
				'在本地运行 Anda：让 Hippocampus 记住上下文，让长程目标、Claude Code、Codex、子智能体 和 IM 工作流持续推进。'
		},
		nav: { install: '安装', reasoning: '推理', memory: '记忆', surfaces: '场景' },
		language: { label: '语言' },
		hero: {
			badge: '开源 Rust 终端 Agent',
			eyebrow: '知识图谱记忆 · 子智能体 · 外部工具',
			title: '安装 Anda。把需要记忆、工具和时间的工作交给它。',
			body: '我是会在工作中持续学习的本地 Agent。Hippocampus 会把经验沉淀成知识图谱记忆，长程目标可以跨越上下文边界继续推进，子智能体还能协同 Claude Code、Codex 等外部 skills 工具。',
			installFor: '安装 {os} 版本',
			seeReasoning: '查看核心循环',
			proofOs: 'Rust',
			proofOsText: '开源终端',
			proofReasoning: '长程推理',
			proofReasoningText: '目标循环',
			proofMemory: '知识图谱',
			proofMemoryText: '长期记忆'
		},
		install: {
			eyebrow: '安装最新版本',
			title: '在本地运行 Anda',
			detected: '已识别 {os}',
			chooseOs: '选择系统',
			tabAria: '按操作系统选择安装方式',
			copy: '复制',
			copied: '已复制',
			copyFailed: '复制失败',
			copyAria: '复制安装命令',
			commandAria: '点击复制安装命令',
			alternative: '备选：{method}',
			steps: ['安装发布版', '配置或导出密钥', '运行'],
			requiresPrefix: '至少需要一个配置文件或环境变量中的模型服务商 API key。Anda 首次启动时会创建',
			requiresSuffix: '配置文件。',
			options: {
				macos: {
					label: 'macOS',
					commandLabel: 'Shell 脚本',
					note: '安装脚本会为 macOS 获取最新发布版和精选 skills。',
					fallbackLabel: 'Homebrew'
				},
				windows: {
					label: 'Windows',
					commandLabel: 'PowerShell',
					note: '在 PowerShell 中运行该命令，然后打开新的终端启动 Anda。'
				},
				linux: {
					label: 'Linux',
					commandLabel: 'Shell 脚本',
					note: '安装脚本会为你的本地运行环境获取最新发布版。'
				}
			}
		},
		reasoning: {
			badge: '长程推理',
			title: '一个目标可以在普通聊天停下之后继续向前推进。',
			body: 'Anda 可以让同一个 session 会话跨越多个关联 conversation 对话保持活跃。它会压缩当前状态、保留目标，让子智能体检查或继续专门工作，并调用真正需要的工具直到结果可验证。',
			panelTitle: 'Anda 会话循环',
			panelStatus: 'goal://active',
			phases: ['推理和执行', '压缩和接力', '评估和继续'],
			signals: [
				{ label: '目标', value: '进行中', level: 92 },
				{ label: '子智能体', value: '协同中', level: 84 },
				{ label: '工具', value: 'Claude Code / Codex', level: 76 },
				{ label: '记忆', value: '形成中', level: 68 }
			],
			events: [
				{ time: '00:01', phase: '目标', detail: '目标已接收，会话已开启' },
				{ time: '47:18', phase: '工具', detail: '检查外部编码工具、文件和运行结果' },
				{ time: '长程', phase: '审查', detail: '子智能体 持续推进直到证据完整' }
			],
			cards: [
				{
					label: '长程目标',
					title: '一次回答不够时，它会继续工作',
					detail:
						'给 Anda 一个目标，它可以检查进展、压缩上下文、进入关联 conversation，并一直推进到结果真正落地。'
				},
				{
					label: '子智能体',
					title: '拆给专门角色，但不丢主线',
					detail:
						'专门的子智能体可以研究、实现、审查或监督，主会话继续保留整体计划、记忆线索和最终目标。'
				},
				{
					label: '工具调用',
					title: '会用你机器上已有的工具',
					detail:
						'Anda 可以调用 shell 和文件工具，加载 Skills，并在任务需要时协同 Claude Code、Codex 等外部编码工具。'
				}
			]
		},
		memory: {
			badge: 'Anda Hippocampus',
			title: '记忆是一张会持续学习精华的知识图谱。',
			body: 'Hippocampus 会把有用的对话转化为活的 Cognitive Nexus：人物、项目、决策、偏好、事件、时间线，以及它们之间的关系。',
			formationTitle: '形成',
			formationBody:
				'我会把成功的对话轮次、文件、频道上下文和工具结果提炼成带来源、时间和意义的记忆。',
			recallTitle: '召回',
			recallBody:
				'当目标需要历史上下文时，我会召回能解释当前任务的关系和时间线，而不仅是匹配相近文本。',
			maintenanceTitle: '维护',
			maintenanceBody: '我会合并、巩固、衰减并保留时间线，让重要知识始终可达，旧噪声自然淡出。'
		},
		work: {
			contextRoutes: '上下文路径',
			memoryRoute: 'anda://memory',
			badge: '工作场景',
			title: '把长记忆带到终端、工具和团队频道。',
			body: '我作为开源 Rust 终端 Agent 运行，也可以接入微信、飞书/Lark、Telegram、Discord、IRC 等 IM，并让同一条记忆线索贯穿工具调用和子智能体。',
			surfaces: [
				{ label: '终端', detail: '本地 Rust 运行时让命令、文件和 Agent 工作靠近你的机器。' },
				{
					label: '外部工具',
					detail: 'Claude Code、Codex、shell、Skills 和文件都可以进入同一个工作循环。'
				},
				{ label: '子智能体', detail: '专门角色可以研究、实现、审查，并继续推进复杂任务。' },
				{
					label: 'IM 频道',
					detail: '微信、飞书/Lark、Telegram、Discord、IRC 可以共享同一条记忆线索。'
				}
			],
			cards: [
				{
					title: 'Rust 终端运行时',
					detail: '开源、本地优先，适合命令、文件、实验和多步骤自动化。'
				},
				{ title: '微信、飞书、Telegram', detail: 'IM 中围绕工作的对话可以进入同一条持久上下文。' },
				{
					title: 'Claude Code 和 Codex',
					detail: '把外部编码助手当作工具使用，同时由 Anda 保留目标和记忆。'
				},
				{ title: '强大的 子智能体', detail: '把专门工作和监督拆出去，但不打散项目上下文。' }
			]
		},
		start: {
			badge: '安装之后',
			title: '带上模型密钥，就可以在任意终端运行 Anda。',
			bodyPrefix: '第一次运行可以临时使用环境变量，也可以把 provider 密钥保存到',
			bodySuffix: '供后续会话使用。',
			quickStart: '快速开始',
			meetHippocampus: '了解 Hippocampus',
			terminalLabel: '运行命令',
			sourceComment: '用环境变量立即启动：',
			goalComment: '或把 api_key 保存到 config.yaml 后运行：',
			localRuntime: 'Rust 运行时',
			durableThread: '子智能体',
			inspectableBrain: '图谱大脑'
		}
	},
	es: {
		meta: {
			title: 'Instala Anda Bot - Agente Rust con memoria en grafo',
			description:
				'Instala Anda Bot, un agente Rust open source con memoria larga en grafo, razonamiento prolongado, herramientas externas, subagents e integraciones IM.',
			ogTitle: 'Instala Anda Bot - Agente Rust con memoria en grafo',
			ogDescription:
				'Ejecuta Anda localmente con memoria Hippocampus, objetivos largos, Claude Code, Codex, subagents y flujos de terminal o IM.'
		},
		nav: {
			install: 'Instalar',
			reasoning: 'Razonamiento',
			memory: 'Memoria',
			surfaces: 'Entornos'
		},
		language: { label: 'Idioma' },
		hero: {
			badge: 'Agente Rust de terminal open source',
			eyebrow: 'memoria en grafo · subagents · herramientas externas',
			title: 'Instala Anda. Dale trabajo que necesita memoria, herramientas y tiempo.',
			body: 'Soy el agente local que sigue aprendiendo mientras trabajamos. Hippocampus convierte experiencia en memoria en grafo, los objetivos largos cruzan límites de contexto y los subagents coordinan herramientas como Claude Code y Codex.',
			installFor: 'Instalar para {os}',
			seeReasoning: 'Ver ciclo central',
			proofOs: 'Rust',
			proofOsText: 'open source',
			proofReasoning: 'largo',
			proofReasoningText: 'objetivos',
			proofMemory: 'grafo',
			proofMemoryText: 'memoria'
		},
		install: {
			eyebrow: 'Instala la última versión',
			title: 'Ejecuta Anda localmente',
			detected: '{os} detectado',
			chooseOs: 'elige SO',
			tabAria: 'Método de instalación por sistema operativo',
			copy: 'Copiar',
			copied: 'Copiado',
			copyFailed: 'No se pudo copiar',
			copyAria: 'Copiar comando de instalación',
			commandAria: 'Copiar el comando de instalación',
			alternative: 'Alternativa: {method}',
			steps: ['Instala la versión', 'Añade o exporta la clave', 'Ejecuta'],
			requiresPrefix:
				'Requiere al menos una clave API de proveedor de modelos en config o env. Anda crea',
			requiresSuffix: 'en el primer inicio.',
			options: {
				macos: {
					label: 'macOS',
					commandLabel: 'Script de shell',
					note: 'El script instala la última versión y las skills seleccionadas para macOS.',
					fallbackLabel: 'Homebrew'
				},
				windows: {
					label: 'Windows',
					commandLabel: 'PowerShell',
					note: 'Ejecuta esto en PowerShell, luego abre una terminal nueva e inicia Anda.'
				},
				linux: {
					label: 'Linux',
					commandLabel: 'Script de shell',
					note: 'El script de instalación descarga la última versión para tu entorno local.'
				}
			}
		},
		reasoning: {
			badge: 'Razonamiento de largo horizonte',
			title: 'Un objetivo puede seguir avanzando cuando un chat normal ya se habría detenido.',
			body: 'Anda mantiene viva una sesión a través de conversaciones enlazadas. Compacta el estado, conserva el objetivo, pide a subagents trabajo focalizado y llama las herramientas necesarias para llegar a un resultado verificable.',
			panelTitle: 'Bucle de sesión de Anda',
			panelStatus: 'goal://active',
			phases: ['razonar', 'compactar', 'continuar'],
			signals: [
				{ label: 'objetivo', value: 'activo', level: 92 },
				{ label: 'subagents', value: 'coordinando', level: 84 },
				{ label: 'herramientas', value: 'Claude Code / Codex', level: 76 },
				{ label: 'memoria', value: 'formándose', level: 68 }
			],
			events: [
				{ time: '00:01', phase: 'objetivo', detail: 'objetivo aceptado y sesión abierta' },
				{
					time: '47:18',
					phase: 'tools',
					detail: 'herramientas externas y archivos inspeccionados'
				},
				{ time: 'largo', phase: 'auditar', detail: 'subagents continúan hasta completar evidencia' }
			],
			cards: [
				{
					label: 'Objetivos largos',
					title: 'Sigue cuando una respuesta no basta',
					detail:
						'Dale a Anda un objetivo y puede revisar progreso, compactar contexto, continuar en una conversación enlazada y avanzar hasta un resultado real.'
				},
				{
					label: 'Subagents',
					title: 'Delega sin perder el hilo principal',
					detail:
						'Subagents especializados pueden investigar, implementar, revisar o supervisar mientras la sesión principal conserva plan, memoria y objetivo.'
				},
				{
					label: 'Herramientas',
					title: 'Usa lo que ya está en tu máquina',
					detail:
						'Anda puede llamar shell, archivos, skills y herramientas externas de código como Claude Code y Codex cuando la tarea lo necesita.'
				}
			]
		},
		memory: {
			badge: 'Anda Hippocampus',
			title: 'La memoria es un grafo que aprende lo útil.',
			body: 'Hippocampus convierte conversaciones útiles en un Cognitive Nexus vivo: personas, proyectos, decisiones, preferencias, eventos, líneas de tiempo y relaciones.',
			formationTitle: 'Formación',
			formationBody:
				'Destilo turnos exitosos, archivos, contexto de canales y resultados de herramientas en recuerdos con fuente, tiempo y significado.',
			recallTitle: 'Recuerdo',
			recallBody:
				'Cuando un objetivo necesita historia, recupero relaciones y líneas de tiempo que explican la tarea actual, no solo texto cercano.',
			maintenanceTitle: 'Mantenimiento',
			maintenanceBody:
				'Consolido, fusiono, reduzco y preservo líneas de tiempo para que el conocimiento durable siga accesible mientras el ruido viejo se desvanece.'
		},
		work: {
			contextRoutes: 'rutas de contexto',
			memoryRoute: 'anda://memory',
			badge: 'Dónde trabajo',
			title: 'Lleva memoria larga a la terminal, herramientas y canales de equipo.',
			body: 'Me ejecuto como agente Rust open source en la terminal, me uno a WeChat, Feishu/Lark, Telegram, Discord e IRC, y mantengo el mismo hilo de memoria entre herramientas y subagents.',
			surfaces: [
				{
					label: 'terminal',
					detail:
						'Un runtime Rust local mantiene comandos, archivos y trabajo de agente cerca de tu máquina.'
				},
				{
					label: 'herramientas',
					detail: 'Claude Code, Codex, shell, skills y archivos pueden entrar en el mismo ciclo.'
				},
				{
					label: 'subagents',
					detail:
						'Trabajadores enfocados investigan, implementan, auditan y continúan tareas complejas.'
				},
				{
					label: 'canales IM',
					detail: 'WeChat, Feishu/Lark, Telegram, Discord e IRC comparten el mismo hilo de memoria.'
				}
			],
			cards: [
				{
					title: 'Runtime Rust de terminal',
					detail:
						'Open source, local-first y pensado para comandos, archivos, experimentos y automatización.'
				},
				{
					title: 'WeChat, Feishu, Telegram',
					detail: 'Las conversaciones IM del trabajo alimentan el mismo contexto durable.'
				},
				{
					title: 'Claude Code y Codex',
					detail:
						'Usa asistentes externos como herramientas mientras Anda conserva objetivo y memoria.'
				},
				{
					title: 'Subagents potentes',
					detail:
						'Delega trabajo especializado y supervisión sin dispersar el contexto del proyecto.'
				}
			]
		},
		start: {
			badge: 'Después de instalar',
			title: 'Ejecuta Anda desde cualquier terminal con una clave de modelo.',
			bodyPrefix: 'Usa una clave de entorno en el primer arranque, o guarda claves en',
			bodySuffix: 'para sesiones futuras.',
			quickStart: 'Inicio rápido',
			meetHippocampus: 'Conocer Hippocampus',
			terminalLabel: 'comando de ejecución',
			sourceComment: 'Inicia ahora con una clave de entorno:',
			goalComment: 'O guarda api_key en config.yaml y ejecuta:',
			localRuntime: 'runtime Rust',
			durableThread: 'subagents',
			inspectableBrain: 'grafo memoria'
		}
	},
	fr: {
		meta: {
			title: 'Installer Anda Bot - Agent Rust à mémoire graphe',
			description:
				'Installez Anda Bot, un agent Rust open source avec mémoire longue en graphe, raisonnement prolongé, outils externes, subagents et intégrations IM.',
			ogTitle: 'Installer Anda Bot - Agent Rust à mémoire graphe',
			ogDescription:
				'Exécutez Anda localement avec mémoire Hippocampus, objectifs longs, Claude Code, Codex, subagents et workflows terminal ou IM.'
		},
		nav: {
			install: 'Installer',
			reasoning: 'Raisonnement',
			memory: 'Mémoire',
			surfaces: 'Espaces'
		},
		language: { label: 'Langue' },
		hero: {
			badge: 'Agent terminal Rust open source',
			eyebrow: 'mémoire graphe · subagents · outils externes',
			title: 'Installez Anda. Confiez-lui le travail qui demande mémoire, outils et temps.',
			body: 'Je suis l’agent local qui continue d’apprendre pendant que nous travaillons. Hippocampus transforme l’expérience en mémoire graphe, les objectifs longs traversent les limites de contexte, et les subagents coordonnent Claude Code, Codex et d’autres outils.',
			installFor: 'Installer pour {os}',
			seeReasoning: 'Voir la boucle',
			proofOs: 'Rust',
			proofOsText: 'open source',
			proofReasoning: 'long',
			proofReasoningText: 'objectifs',
			proofMemory: 'graphe',
			proofMemoryText: 'mémoire'
		},
		install: {
			eyebrow: 'Installer la dernière version',
			title: 'Exécuter Anda localement',
			detected: '{os} détecté',
			chooseOs: 'choisir OS',
			tabAria: 'Méthode d’installation par système d’exploitation',
			copy: 'Copier',
			copied: 'Copié',
			copyFailed: 'Copie échouée',
			copyAria: 'Copier la commande d’installation',
			commandAria: 'Copier la commande d’installation',
			alternative: 'Alternative : {method}',
			steps: ['Installer la version', 'Ajouter ou exporter la clé', 'Lancer'],
			requiresPrefix:
				'Nécessite au moins une clé API de fournisseur de modèle en config ou env. Anda crée',
			requiresSuffix: 'au premier lancement.',
			options: {
				macos: {
					label: 'macOS',
					commandLabel: 'Script shell',
					note: 'Le script installe la dernière version et les skills sélectionnés pour macOS.',
					fallbackLabel: 'Homebrew'
				},
				windows: {
					label: 'Windows',
					commandLabel: 'PowerShell',
					note: 'Exécutez cette commande dans PowerShell, puis ouvrez un nouveau terminal et lancez Anda.'
				},
				linux: {
					label: 'Linux',
					commandLabel: 'Script shell',
					note: 'Le script d’installation récupère la dernière version pour votre runtime local.'
				}
			}
		},
		reasoning: {
			badge: 'Raisonnement de longue durée',
			title: 'Un objectif peut continuer quand un chat ordinaire se serait arrêté.',
			body: 'Anda garde une session active à travers des conversations liées. Il compacte l’état, préserve l’objectif, demande aux subagents un travail ciblé et appelle les outils nécessaires pour atteindre un résultat vérifiable.',
			panelTitle: 'Boucle de session Anda',
			panelStatus: 'goal://active',
			phases: ['raisonner', 'compacter', 'continuer'],
			signals: [
				{ label: 'objectif', value: 'actif', level: 92 },
				{ label: 'subagents', value: 'coordonnent', level: 84 },
				{ label: 'outils', value: 'Claude Code / Codex', level: 76 },
				{ label: 'mémoire', value: 'en formation', level: 68 }
			],
			events: [
				{ time: '00:01', phase: 'objectif', detail: 'objectif accepté et session ouverte' },
				{ time: '47:18', phase: 'outils', detail: 'outils externes et fichiers inspectés' },
				{ time: 'long', phase: 'audit', detail: 'les subagents continuent jusqu’aux preuves' }
			],
			cards: [
				{
					label: 'Objectifs longs',
					title: 'Continue quand une réponse ne suffit pas',
					detail:
						'Donnez un objectif à Anda : il vérifie la progression, compacte le contexte, continue dans une conversation liée et avance jusqu’au résultat réel.'
				},
				{
					label: 'Subagents',
					title: 'Délègue sans perdre le fil principal',
					detail:
						'Des subagents spécialisés peuvent rechercher, implémenter, relire ou superviser pendant que la session principale garde plan, mémoire et objectif.'
				},
				{
					label: 'Outils',
					title: 'Utilise les outils déjà présents sur votre machine',
					detail:
						'Anda peut appeler shell, fichiers, skills et outils de code externes comme Claude Code et Codex quand la tâche l’exige.'
				}
			]
		},
		memory: {
			badge: 'Anda Hippocampus',
			title: 'La mémoire est un graphe qui apprend l’essentiel.',
			body: 'Hippocampus transforme les conversations utiles en Cognitive Nexus vivant : personnes, projets, décisions, préférences, événements, chronologies et relations.',
			formationTitle: 'Formation',
			formationBody:
				'Je distille les tours réussis, fichiers, contextes de canaux et résultats d’outils en souvenirs avec source, temps et sens.',
			recallTitle: 'Rappel',
			recallBody:
				'Quand un objectif a besoin d’historique, je récupère relations et chronologies qui expliquent la tâche actuelle, pas seulement le texte proche.',
			maintenanceTitle: 'Maintenance',
			maintenanceBody:
				'Je consolide, fusionne, fais décroître et préserve les chronologies afin que le savoir durable reste accessible pendant que le bruit ancien s’efface.'
		},
		work: {
			contextRoutes: 'routes de contexte',
			memoryRoute: 'anda://memory',
			badge: 'Où je travaille',
			title: 'Apportez la mémoire longue au terminal, aux outils et aux canaux.',
			body: 'Je fonctionne comme agent terminal Rust open source, rejoins WeChat, Feishu/Lark, Telegram, Discord et IRC, et garde le même fil mémoire entre outils et subagents.',
			surfaces: [
				{
					label: 'terminal',
					detail:
						'Un runtime Rust local garde commandes, fichiers et travail agent près de votre machine.'
				},
				{
					label: 'outils',
					detail: 'Claude Code, Codex, shell, skills et fichiers peuvent rejoindre la même boucle.'
				},
				{
					label: 'subagents',
					detail:
						'Des travailleurs ciblés recherchent, implémentent, auditent et poursuivent les tâches complexes.'
				},
				{
					label: 'canaux IM',
					detail: 'WeChat, Feishu/Lark, Telegram, Discord et IRC partagent le même fil mémoire.'
				}
			],
			cards: [
				{
					title: 'Runtime terminal Rust',
					detail:
						'Open source, local-first, pensé pour commandes, fichiers, expériences et automatisation.'
				},
				{
					title: 'WeChat, Feishu, Telegram',
					detail: 'Les conversations IM autour du travail nourrissent le même contexte durable.'
				},
				{
					title: 'Claude Code et Codex',
					detail:
						'Utilisez des assistants externes comme outils pendant qu’Anda garde objectif et mémoire.'
				},
				{
					title: 'Subagents puissants',
					detail: 'Déléguez travail spécialisé et supervision sans disperser le contexte du projet.'
				}
			]
		},
		start: {
			badge: 'Après installation',
			title: 'Lancez Anda depuis n’importe quel terminal avec une clé modèle.',
			bodyPrefix:
				'Utilisez une clé en variable d’environnement au premier lancement, ou gardez les clés dans',
			bodySuffix: 'pour les sessions suivantes.',
			quickStart: 'Démarrage rapide',
			meetHippocampus: 'Découvrir Hippocampus',
			terminalLabel: 'commande',
			sourceComment: 'Démarrer avec une clé d’environnement :',
			goalComment: 'Ou enregistrer api_key dans config.yaml, puis lancer :',
			localRuntime: 'runtime Rust',
			durableThread: 'subagents',
			inspectableBrain: 'graphe mémoire'
		}
	},
	ru: {
		meta: {
			title: 'Установите Anda Bot - Rust-агент с графовой памятью',
			description:
				'Установите Anda Bot: open-source Rust-агент с долгой графовой памятью, длительным рассуждением, внешними инструментами, subagents и IM-интеграциями.',
			ogTitle: 'Установите Anda Bot - Rust-агент с графовой памятью',
			ogDescription:
				'Запускайте Anda локально с памятью Hippocampus, долгими целями, Claude Code, Codex, subagents и workflow в терминале или IM.'
		},
		nav: { install: 'Установка', reasoning: 'Рассуждение', memory: 'Память', surfaces: 'Среды' },
		language: { label: 'Язык' },
		hero: {
			badge: 'Open-source Rust-агент для терминала',
			eyebrow: 'графовая память · subagents · внешние инструменты',
			title: 'Установите Anda. Дайте работу, где нужны память, инструменты и время.',
			body: 'Я локальный агент, который продолжает учиться во время работы. Hippocampus превращает опыт в графовую память, долгие цели проходят границы контекста, а subagents координируют Claude Code, Codex и другие инструменты.',
			installFor: 'Установить для {os}',
			seeReasoning: 'Смотреть цикл',
			proofOs: 'Rust',
			proofOsText: 'open source',
			proofReasoning: 'долго',
			proofReasoningText: 'цели',
			proofMemory: 'граф',
			proofMemoryText: 'память'
		},
		install: {
			eyebrow: 'Установите последнюю версию',
			title: 'Запустите Anda локально',
			detected: 'обнаружено: {os}',
			chooseOs: 'выберите OS',
			tabAria: 'Способ установки по операционной системе',
			copy: 'Копировать',
			copied: 'Скопировано',
			copyFailed: 'Не удалось скопировать',
			copyAria: 'Скопировать команду установки',
			commandAria: 'Скопировать команду установки',
			alternative: 'Альтернатива: {method}',
			steps: ['Установите релиз', 'Добавьте или экспортируйте ключ', 'Запустите'],
			requiresPrefix:
				'Нужен хотя бы один API-ключ провайдера модели в config или env. При первом запуске Anda создает',
			requiresSuffix: '.',
			options: {
				macos: {
					label: 'macOS',
					commandLabel: 'Shell-скрипт',
					note: 'Скрипт установки загружает последний релиз и выбранные skills для macOS.',
					fallbackLabel: 'Homebrew'
				},
				windows: {
					label: 'Windows',
					commandLabel: 'PowerShell',
					note: 'Запустите это в PowerShell, затем откройте новый терминал и стартуйте Anda.'
				},
				linux: {
					label: 'Linux',
					commandLabel: 'Shell-скрипт',
					note: 'Скрипт установки скачает последний релиз для вашей локальной среды.'
				}
			}
		},
		reasoning: {
			badge: 'Долгое рассуждение',
			title: 'Цель может двигаться дальше, когда обычный чат уже остановился бы.',
			body: 'Anda поддерживает одну сессию через связанные разговоры. Он сжимает состояние, сохраняет цель, поручает subagents точечную работу и вызывает нужные инструменты, чтобы дойти до проверяемого результата.',
			panelTitle: 'Цикл сессии Anda',
			panelStatus: 'goal://active',
			phases: ['рассуждать', 'сжимать', 'продолжать'],
			signals: [
				{ label: 'цель', value: 'активна', level: 92 },
				{ label: 'subagents', value: 'координация', level: 84 },
				{ label: 'инструменты', value: 'Claude Code / Codex', level: 76 },
				{ label: 'память', value: 'формируется', level: 68 }
			],
			events: [
				{ time: '00:01', phase: 'цель', detail: 'цель принята, сессия открыта' },
				{ time: '47:18', phase: 'tools', detail: 'внешние инструменты и файлы проверены' },
				{ time: 'долго', phase: 'аудит', detail: 'subagents продолжают до полной проверки' }
			],
			cards: [
				{
					label: 'Долгие цели',
					title: 'Продолжает, когда одного ответа мало',
					detail:
						'Дайте Anda цель: он проверяет прогресс, сжимает контекст, продолжает в связанном разговоре и идет до реального результата.'
				},
				{
					label: 'Subagents',
					title: 'Делегирует, не теряя главную нить',
					detail:
						'Специализированные subagents могут исследовать, реализовывать, проверять или надзирать, пока главная сессия хранит план, память и цель.'
				},
				{
					label: 'Инструменты',
					title: 'Использует то, что уже есть на машине',
					detail:
						'Anda вызывает shell, файлы, skills и внешние coding tools вроде Claude Code и Codex, когда задача этого требует.'
				}
			]
		},
		memory: {
			badge: 'Anda Hippocampus',
			title: 'Память - это граф, который учит полезное.',
			body: 'Hippocampus превращает полезные разговоры в живой Cognitive Nexus: людей, проекты, решения, предпочтения, события, линии времени и отношения.',
			formationTitle: 'Формирование',
			formationBody:
				'Я выделяю успешные ходы, файлы, контекст каналов и результаты инструментов в воспоминания с источником, временем и смыслом.',
			recallTitle: 'Вызов',
			recallBody:
				'Когда цели нужна история, я извлекаю связи и линии времени, объясняющие задачу, а не просто ближайший текст.',
			maintenanceTitle: 'Поддержка',
			maintenanceBody:
				'Я консолидирую, объединяю, ослабляю и сохраняю линии времени, чтобы прочное знание оставалось доступным, а старый шум исчезал.'
		},
		work: {
			contextRoutes: 'маршруты контекста',
			memoryRoute: 'anda://memory',
			badge: 'Где я работаю',
			title: 'Принесите долгую память в терминал, инструменты и каналы команды.',
			body: 'Я работаю как open-source Rust-агент в терминале, подключаюсь к WeChat, Feishu/Lark, Telegram, Discord и IRC и сохраняю одну нить памяти между инструментами и subagents.',
			surfaces: [
				{
					label: 'терминал',
					detail: 'Локальный Rust runtime держит команды, файлы и работу агента рядом с машиной.'
				},
				{
					label: 'инструменты',
					detail: 'Claude Code, Codex, shell, skills и файлы входят в один рабочий цикл.'
				},
				{
					label: 'subagents',
					detail: 'Фокусные работники исследуют, реализуют, проверяют и продолжают сложные задачи.'
				},
				{
					label: 'IM-каналы',
					detail: 'WeChat, Feishu/Lark, Telegram, Discord и IRC разделяют одну нить памяти.'
				}
			],
			cards: [
				{
					title: 'Rust runtime в терминале',
					detail:
						'Open source, local-first, для команд, файлов, экспериментов и многошаговой автоматизации.'
				},
				{
					title: 'WeChat, Feishu, Telegram',
					detail: 'IM-разговоры о работе питают тот же долговечный контекст.'
				},
				{
					title: 'Claude Code и Codex',
					detail:
						'Используйте внешних coding assistants как инструменты, пока Anda хранит цель и память.'
				},
				{
					title: 'Сильные subagents',
					detail: 'Делегируйте специализированную работу и надзор, не распыляя контекст проекта.'
				}
			]
		},
		start: {
			badge: 'После установки',
			title: 'Запускайте Anda из любого терминала с ключом модели.',
			bodyPrefix:
				'Для первого запуска используйте ключ в переменной окружения или сохраните ключи в',
			bodySuffix: 'для следующих сессий.',
			quickStart: 'Быстрый старт',
			meetHippocampus: 'Познакомиться с Hippocampus',
			terminalLabel: 'команда запуска',
			sourceComment: 'Запустить сразу с ключом окружения:',
			goalComment: 'Или сохраните api_key в config.yaml и запустите:',
			localRuntime: 'Rust runtime',
			durableThread: 'subagents',
			inspectableBrain: 'граф памяти'
		}
	},
	ar: {
		meta: {
			title: 'ثبّت Anda Bot - وكيل Rust بذاكرة رسومية',
			description:
				'ثبّت Anda Bot، وكيل Rust مفتوح المصدر بذاكرة طويلة رسومية، واستدلال ممتد، وأدوات خارجية، و subagents، وتكاملات IM.',
			ogTitle: 'ثبّت Anda Bot - وكيل Rust بذاكرة رسومية',
			ogDescription:
				'شغّل Anda محليًا مع ذاكرة Hippocampus، وأهداف طويلة، و Claude Code، و Codex، و subagents، وسير عمل في الطرفية أو IM.'
		},
		nav: { install: 'التثبيت', reasoning: 'الاستدلال', memory: 'الذاكرة', surfaces: 'بيئات العمل' },
		language: { label: 'اللغة' },
		hero: {
			badge: 'وكيل طرفية Rust مفتوح المصدر',
			eyebrow: 'ذاكرة رسومية · subagents · أدوات خارجية',
			title: 'ثبّت Anda. أعطه عملًا يحتاج ذاكرة وأدوات ووقتًا.',
			body: 'أنا الوكيل المحلي الذي يواصل التعلّم أثناء العمل. يحوّل Hippocampus الخبرة إلى ذاكرة رسومية، وتستمر الأهداف الطويلة عبر حدود السياق، وتنسّق subagents أدوات مثل Claude Code و Codex.',
			installFor: 'ثبّت لـ {os}',
			seeReasoning: 'اعرض الحلقة',
			proofOs: 'Rust',
			proofOsText: 'مفتوح المصدر',
			proofReasoning: 'طويل',
			proofReasoningText: 'أهداف',
			proofMemory: 'رسم بياني',
			proofMemoryText: 'ذاكرة'
		},
		install: {
			eyebrow: 'ثبّت أحدث إصدار',
			title: 'شغّل Anda محليًا',
			detected: 'تم اكتشاف {os}',
			chooseOs: 'اختر النظام',
			tabAria: 'طريقة التثبيت حسب نظام التشغيل',
			copy: 'نسخ',
			copied: 'تم النسخ',
			copyFailed: 'تعذّر النسخ',
			copyAria: 'نسخ أمر التثبيت',
			commandAria: 'انسخ أمر التثبيت',
			alternative: 'بديل: {method}',
			steps: ['ثبّت الإصدار', 'أضف المفتاح أو صدّره', 'شغّل'],
			requiresPrefix: 'يتطلب مفتاح API واحدًا على الأقل لمزوّد نموذج في config أو env. ينشئ Anda',
			requiresSuffix: 'عند أول تشغيل.',
			options: {
				macos: {
					label: 'macOS',
					commandLabel: 'سكربت Shell',
					note: 'يثبت سكربت التثبيت أحدث إصدار و skills المنتقاة على macOS.',
					fallbackLabel: 'Homebrew'
				},
				windows: {
					label: 'Windows',
					commandLabel: 'PowerShell',
					note: 'شغّل هذا في PowerShell، ثم افتح طرفية جديدة وابدأ Anda.'
				},
				linux: {
					label: 'Linux',
					commandLabel: 'سكربت Shell',
					note: 'يجلب سكربت التثبيت أحدث إصدار لبيئة التشغيل المحلية لديك.'
				}
			}
		},
		reasoning: {
			badge: 'استدلال طويل المدى',
			title: 'يمكن للهدف أن يواصل الحركة عندما تتوقف المحادثة العادية.',
			body: 'يبقي Anda الجلسة حيّة عبر محادثات مرتبطة. يضغط الحالة، ويحافظ على الهدف، ويطلب من subagents عملًا مركّزًا، ويستدعي الأدوات اللازمة للوصول إلى نتيجة قابلة للتحقق.',
			panelTitle: 'حلقة جلسة Anda',
			panelStatus: 'goal://active',
			phases: ['استدلال', 'ضغط', 'متابعة'],
			signals: [
				{ label: 'الهدف', value: 'نشط', level: 92 },
				{ label: 'subagents', value: 'تنسّق', level: 84 },
				{ label: 'الأدوات', value: 'Claude Code / Codex', level: 76 },
				{ label: 'الذاكرة', value: 'تتشكل', level: 68 }
			],
			events: [
				{ time: '00:01', phase: 'هدف', detail: 'قُبل الهدف وفتحت الجلسة' },
				{ time: '47:18', phase: 'أدوات', detail: 'فُحصت أدوات خارجية وملفات' },
				{ time: 'طويل', phase: 'تدقيق', detail: 'تستمر subagents حتى تكتمل الأدلة' }
			],
			cards: [
				{
					label: 'أهداف طويلة',
					title: 'يستمر عندما لا تكفي إجابة واحدة',
					detail:
						'امنح Anda هدفًا، فيفحص التقدم، ويضغط السياق، ويتابع في محادثة مرتبطة، ويتحرك حتى تصبح النتيجة حقيقية.'
				},
				{
					label: 'Subagents',
					title: 'يفوّض العمل دون فقدان الخيط الرئيسي',
					detail:
						'يمكن لـ subagents متخصصة البحث والتنفيذ والمراجعة والإشراف بينما تحفظ الجلسة الرئيسية الخطة والذاكرة والهدف.'
				},
				{
					label: 'الأدوات',
					title: 'يستخدم ما هو موجود على جهازك',
					detail:
						'يمكن لـ Anda استدعاء shell والملفات و skills وأدوات البرمجة الخارجية مثل Claude Code و Codex عند الحاجة.'
				}
			]
		},
		memory: {
			badge: 'Anda Hippocampus',
			title: 'الذاكرة رسم بياني يتعلم الخلاصة المفيدة.',
			body: 'يحوّل Hippocampus المحادثات المفيدة إلى Cognitive Nexus حي: أشخاص، ومشاريع، وقرارات، وتفضيلات، وأحداث، وجداول زمنية، وعلاقات.',
			formationTitle: 'التكوين',
			formationBody:
				'أستخلص الجولات الناجحة والملفات وسياق القنوات ونتائج الأدوات إلى ذكريات لها مصدر ووقت ومعنى.',
			recallTitle: 'الاسترجاع',
			recallBody:
				'عندما يحتاج الهدف إلى تاريخ، أسترجع العلاقات والجداول الزمنية التي تشرح المهمة، لا أقرب نص فقط.',
			maintenanceTitle: 'الصيانة',
			maintenanceBody:
				'أدمج وأوحّد وأخفّض الضجيج وأحافظ على الجداول الزمنية كي تبقى المعرفة الدائمة قابلة للوصول بينما يتلاشى الضجيج القديم.'
		},
		work: {
			contextRoutes: 'مسارات السياق',
			memoryRoute: 'anda://memory',
			badge: 'أين أعمل',
			title: 'اجلب الذاكرة الطويلة إلى الطرفية والأدوات وقنوات الفريق.',
			body: 'أعمل كوكيل Rust مفتوح المصدر في الطرفية، وأتصل بـ WeChat و Feishu/Lark و Telegram و Discord و IRC، وأحافظ على خيط ذاكرة واحد بين الأدوات و subagents.',
			surfaces: [
				{
					label: 'الطرفية',
					detail: 'يبقي runtime Rust المحلي الأوامر والملفات وعمل الوكيل قرب جهازك.'
				},
				{
					label: 'الأدوات',
					detail: 'يمكن لـ Claude Code و Codex و shell و skills والملفات دخول الحلقة نفسها.'
				},
				{
					label: 'subagents',
					detail: 'عمال مركّزون يبحثون وينفذون ويدققون ويتابعون المهام المعقدة.'
				},
				{
					label: 'قنوات IM',
					detail: 'تشارك WeChat و Feishu/Lark و Telegram و Discord و IRC خيط الذاكرة نفسه.'
				}
			],
			cards: [
				{
					title: 'Runtime Rust في الطرفية',
					detail: 'مفتوح المصدر ومحلي أولًا للأوامر والملفات والتجارب والأتمتة متعددة الخطوات.'
				},
				{
					title: 'WeChat و Feishu و Telegram',
					detail: 'محادثات IM حول العمل تغذي السياق الدائم نفسه.'
				},
				{
					title: 'Claude Code و Codex',
					detail: 'استخدم مساعدين خارجيين كأدوات بينما يحفظ Anda الهدف والذاكرة.'
				},
				{
					title: 'Subagents قوية',
					detail: 'فوّض العمل المتخصص والإشراف دون تشتيت سياق المشروع.'
				}
			]
		},
		start: {
			badge: 'بعد التثبيت',
			title: 'شغّل Anda من أي طرفية باستخدام مفتاح نموذج.',
			bodyPrefix: 'استخدم مفتاحًا في متغير بيئة عند التشغيل الأول، أو احفظ مفاتيح المزوّد في',
			bodySuffix: 'للجلسات التالية.',
			quickStart: 'البدء السريع',
			meetHippocampus: 'تعرّف على Hippocampus',
			terminalLabel: 'أمر التشغيل',
			sourceComment: 'ابدأ الآن بمفتاح بيئة:',
			goalComment: 'أو احفظ api_key في config.yaml ثم شغّل:',
			localRuntime: 'runtime Rust',
			durableThread: 'subagents',
			inspectableBrain: 'ذاكرة رسومية'
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
