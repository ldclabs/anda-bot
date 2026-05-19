export function scrollIntoView(
	messageId: string,
	behavior: ScrollBehavior = 'instant',
	block: ScrollLogicalPosition = 'center'
): void {
	const ele = document.getElementById(messageId)

	if (ele) {
		ele.scrollIntoView({
			block,
			behavior
		})
	}
}
