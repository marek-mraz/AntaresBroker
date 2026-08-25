*** Settings ***
Documentation       Check that one can retrieve the temporal evolution of the last N instances of entity attributes

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Temporal Entity
Test Teardown       Delete Initial Temporal Entity
Test Template       Retrieve the temporal evolution of the last N instances of entity attributes


*** Variables ***
${vehicle_payload_file}=    2020-08-vehicle-temporal-representation-multiple-instances.jsonld


*** Test Cases ***    LASTN    VEHICLE_EXPECTATION_FILE
020_05_01 Retrieve Some Instances
    [Tags]    te-retrieve    5_7_3
    ${4}    vehicle-temporal-representation-020-05-01.jsonld
020_05_02 Retrieve All Instances
    [Tags]    te-retrieve    5_7_3
    ${7}    vehicle-temporal-representation-020-05-02.jsonld


*** Keywords ***
Retrieve the temporal evolution of the last N instances of entity attributes
    [Documentation]    Check that one can retrieve the temporal evolution of the last N instances of entity attributes
    [Arguments]    ${lastn}    ${vehicle_expectation_file}
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    lastN=${lastn}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing EntityTemporal element
    ...    ${vehicle_expectation_file}
    ...    ${temporal_entity_representation_id}
    ...    ${response.json()}

Create Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${create_response}=    Create Temporal Representation Of Entity
    ...    ${vehicle_payload_file}
    ...    ${temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Test Variable    ${temporal_entity_representation_id}

Delete Initial Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
