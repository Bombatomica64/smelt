function codeLabel(code: 1 | 2): string {
	switch (code) {
		case 1:
			return "one";
		case 2:
			return "two";
		default:
			return "other";
	}
}

const resu = codeLabel(2);
console.log(resu);
