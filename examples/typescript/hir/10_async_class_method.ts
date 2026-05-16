class Counter {
	value: number;

	constructor(value: number) {
		this.value = value;
	}

	async bump(): Promise<number> {
		this.value = this.value + 1;
		return this.value;
	}
}

async function run(): Promise<number> {
	const counter = new Counter(3);
	return await counter.bump();
}
