# Run ITB locally with Robot testing service

This setup runs ITB + the side service to trigger Robot tests.
Preconfigured with sample Robot tests and MIM1 TDL tests for comparison.

### Start ITB
```cd itb```
```docker compose up --build```

This will run the 4 containers ITB needs (redis, srv, mysql and ui)

- itb-redis is a cache server used for user sessions management.
- itb-mysql is the Test Bed's database
- itb-srv is the Test Bed's backend test engine that executes tests.
- itb-ui is the Test Bed's frontend that users access.

This will also run a service named "robot-processing" which is the external service we developed for this project

Attach your context broker to the ITB network:
```docker network connect itb <broker_name>```

### Access ITB
The ITB UI is available at: http://localhost:9000

The default logs are :
user
change_this_password

### Import Test Suites

By default the official Robot NGSI-LD Test Suite will be imported to ITB
But note that you can also import your own Test Suite if you have forked and modify it for example

You would need to "initialize" again running the command

```curl -iX POST 'http://localhost:8080/api/init' -d '{<link_to_your_repository>}' -H 'Content-type: application/json'```

This will trigger the initialization of the external service.
This will git clone your test suite.
From the test files, the service generate their ITB version and deploy them in ITB.

### Configure your SUT

From here, everything is preconfigured in ITB but the SUT configuration.
Community management -> Robot NGSI-LD Testing Community -> Robot NGSI-LD -> Context Broker

Provide the details of your system and to forget to add the endpoint of it going to Additional properties
Don't forget to save your modifications

### Run Tests
Manage tests -> "Robot NGSI-LD" 

Use the filters to find specific test cases