async function lift(value: number): Promise<number> {
	return value;
}

async function run(): Promise<number> {
	return await lift(5);
}
