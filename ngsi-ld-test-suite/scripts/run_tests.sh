#!/bin/bash

# run all tests
robot --outputdir ./results ./TP/NGSI-LD

# run all tests with base url overriden
robot --variable url:"URL_HERE" --outputdir ./results .

# run by specific tag(s)
robot --include mandatory --outputdir ./results .

# run all the test suites for context information
robot --outputdir ./results ./TP/NGSI-LD/ContextInformation

# run a specific test suite
robot --outputdir ./results ./TP/NGSI-LD/ContextInformation/Provision/Entities/CreateEntity/001_01.robot
robot --outputdir ./results --suite 001_01 .

# run specific test suite
robot --outputdir ./results -t "InvalidId" TP/NGSI-LD/ContextInformation/Subscription/RetrieveSubscription/030_01.robot

# rerun failed test cases
robot --rerunfailedsuites ./results/output.xml  --outputdir ./results .

# stop the suite after a failed test
robot --exitonfailure --outputdir ./results . 

# run specific test suite and generate errors.log file with the output only of the failed test cases
robot  --listener libraries/ErrorListener.py --outputdir ./results ./TP/NGSI-LD/CommonBehaviours/043.robot

# run all test suites and generate errors.log file with the output only of the failed test cases
robot  --listener libraries/ErrorListener.py --outputdir ./results .

#run interoperability test suite with multiple brokers
robot --variable b1_url:<broker1_url> \
      --variable b2_url:<broker2_url> \
      --variable b3_url:<broker3_url> \
      --variable b4_url:<broker4_url> \
      --variable b5_url:<broker5_url> \
      --outputdir ./results ./TP/NGSI-LD/Interoperability

#run specific interoperability test suite with multiple brokers
robot --variable b1_url:<broker1_url> \
      --variable b2_url:<broker2_url> \
      --variable b3_url:<broker3_url> \
      --variable b4_url:<broker4_url> \
      --variable b5_url:<broker5_url> \
      --outputdir ./results ./TP/NGSI-LD/Interoperability/Provision/Entities/CreateEntity/IOP_CNF_01_01.robot
