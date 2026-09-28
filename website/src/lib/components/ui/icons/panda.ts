import pandaSvg from '$lib/assets/panda.svg?raw';

// Subpaths of the Anda panda artwork (500 × 500 units). The artwork's first
// path also carries offset copies of the face features that the head hides;
// only its ears and legs are used here.
const paths = [...pandaSvg.matchAll(/\sd="([^"]+)"/g)].map((match) =>
	match[1].split(/(?=M)/).filter(Boolean)
);

function part(path: number, sub: number) {
	return paths[path]?.[sub] ?? '';
}

export const panda = {
	head: part(1, 0),
	ears: [part(0, 0), part(0, 1)],
	eyes: [part(2, 0), part(2, 1)],
	pupils: [part(2, 2), part(2, 3)],
	muzzle: part(2, 4),
	legs: [part(0, 7), part(0, 8)]
};
