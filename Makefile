.PHONY: init bootstrap harden doctor up down logs smoke

init:
	./scripts/init-env.sh

bootstrap:
	./scripts/bootstrap-ubuntu.sh

harden:
	./scripts/harden-wsl.sh

doctor:
	./scripts/doctor.sh

up:
	./up.sh

down:
	./down.sh

logs:
	./logs.sh

smoke:
	./scripts/smoke-test.sh
