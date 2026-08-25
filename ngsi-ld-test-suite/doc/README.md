# Automatic generation of documentation data (ETSI RGS CIM 013)

This Python code has been developed to facilitate the automatic generation of the test purposes descriptions in a JSON 
format to automatically generate afterward the NGSI-LD Test Purposes Descriptions (ETSI RGS CIM 013) document.
To achieve this purpose, we are defining a full Unit Test Suite with several Test Cases to check that we can generate
the corresponding information of the Robot description files. 

This approach has a clear advantage, from one side, we can generate the corresponding info for the ETSI document and
for the other side, it can help us to know when a change in the Robot Description files was developed and therefore
an update in the ETSI document (new version of the document) must be generated.

## Requirements

This Python code is developed under the scope of the overall NGSI-LD Test Suite Code, therefore we use the same
requirements and python virtual environment already defined in [README.md](../README.md).

## Structure

The unit tests are divided into several groups following the NGSI-LD Test Suite Structure (ETSI RGS CIM 012) divided
in test groups and subgroups:

- Group 1: Context Information (CI), Provision (PROV), defined in the 
[test_ContextInformation_Provision.py](./tests/test_ContextInformation_Provision.py) file.
- Group 2: Context Information (CI), Consumption (CONS), defined in the 
[test_ContextInformation_Consumption.py](./tests/test_ContextInformation_Consumption.py) file.
- Group 3: Context Information (CI), Subscription (SUB), defined in the 
[test_ContextInformation_Subscription.py](./tests/test_ContextInformation_Subscription.py) file.
- Group 4: Context Source (CS), Registration (REG), defined in the 
[test_ContextSource_Registration.py](./tests/test_ContextSource_Registration.py) file.
- Group 5: Context Source (CS), Discovery (DISC), defined in the 
[test_ContextSource_Discovery.py](./tests/test_ContextSource_Discovery.py) file.
- Group 6: Context Source (CS), Registration Subscription (REGSUB), defined in the 
[test_ContextSource_RegistrationSubscription.py](./tests/test_ContextSource_RegistrationSubscription.py) file.
- Group 7: Common Behaviours (CB), defined in the 
[test_CommonBehaviours.py](./tests/test_CommonBehaviours.py) file.
- Group 8: Storing, Managing and Serving @contexts (CTX), Consumption (CONS), defined in the 
[test_jsonldContext_Consumption.py](./tests/test_jsonldContext_Consumption.py) file.
- Group 9: Storing, Managing and Serving @contexts (CTX), Provision (PROV), defined in the 
[test_jsonldContext_Provision.py](./tests/test_jsonldContext_Provision.py) file.

Additionally, a specific unit test called [test_CheckTests.py](./tests/test_CheckTests.py) was created to check that all
robot files have the corresponding unit tests. 

Moreover, the [files](./files) folder contains the expected results of the execution of the Unit Tests, and they are 
divided into the NGSI-LD Test Suite Structure groups:

- [CommonBehaviours](./files/CommonBehaviours) contains the expected results of the Common Behaviours robot files.
- [ContextInformation](./files/ContextInformation) contains the expected results of the Context Information files.
- [ContextSource](./files/ContextSource) contains the expected results of the Context Source files.
- [jsonldContext](./files/jsonldContext) contains the expected results of the Storing, Managing and Serving @contexts files.

## Execution

### Using PyCharm

You have several options to execute the tests. It was generated a set of PyCharm configuration files in the 
[runConfigurations](../.idea/runConfigurations) folder to help in the execution of the unit tests from the IDE:

- [All Unit Tests](../.idea/runConfigurations/All_Unit_Tests.xml) executes all Unit Tests associated to the robot files.
- [CheckTests Unit Tests](../.idea/runConfigurations/CheckTests_Unit_Tests.xml) checks that there is a unit test associated to each robot file.
- [CommonBehaviours Unit Tests](../.idea/runConfigurations/CommonBehaviours_Unit_Tests.xml) executes Common Behaviours unit tests associated to the robot files.
- [ContextInformation Consumption Unit Tests](../.idea/runConfigurations/ContextInformation_Consumption_Unit_Tests.xml) executes Context Information - Consumption unit tests associated to 
the robot files.
- [ContextInformation Provision Unit Tests](../.idea/runConfigurations/ContextInformation_Provision_Unit_Tests.xml) executes Context Information - Provision unit tests associated to the 
robot files.
- [ContextInformation Subscription Unit Tests](../.idea/runConfigurations/ContextInformation_Subscription_Unit_Tests.xml) executes Context Information - Subscription unit tests associated 
to the robot files.
- [ContextSource Discovery Unit Tests](../.idea/runConfigurations/ContextSource_Discovery_Unit_Tests.xml) executes Context Source - Discovery unit tests associated to the robot 
files.
- [ContextSource Registration Unit Tests](../.idea/runConfigurations/ContextSource_Registration_Unit_Tests.xml) executes Context Source - Registration unit tests associated to the 
robot files.
- [ContextSource RegistrationSubscription Unit Tests](../.idea/runConfigurations/ContextSource_RegistrationSubscription_Unit_Tests.xml) executes Context Source - Registration Subscription unit 
tests associated to the robot files.
- [jsonldContext Consumption Unit Tests](../.idea/runConfigurations/jsonldContext_Consumption_Unit_Tests.xml) executes Storing, Managing and Serving @contexts - Consumption unit tests 
associated to the robot files.
- [jsonldContext Provision Unit Tests](../.idea/runConfigurations/jsonldContext_Provision_Unit_Tests.xml) executes Storing, Managing and Serving @contexts - Provision unit tests 
associated to the robot files.
- [Generate Documentation Data](../.idea/runConfigurations/Generate_Documentation_Data.xml) executes Generate Document Data unit tests associated to the unit test execution 
of a specific robot file (e.g. 047_01). The generated files are located in the [results](./results) folder.
- [Statistics Documentation Data](../.idea/runConfigurations/Statistics_Documentation_Data.xml) executes all the tests to generate the documentation and generate statistics about
the execution. The generated files are located in the [results](./results) folder, a special file called `testcases.json`
is generated with a list of all generated data.
