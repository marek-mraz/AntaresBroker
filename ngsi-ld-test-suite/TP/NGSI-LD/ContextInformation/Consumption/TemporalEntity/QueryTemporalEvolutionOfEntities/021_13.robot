*** Settings ***
Documentation       Check that one can query the temporal evolution of entities using the entityOperations method

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Temporal Entities
Test Teardown       Delete Initial Temporal Entities
Test Template       Query the temporal evolution of entities using the entityOperations method


*** Variables ***
${first_vehicle_payload_file}=      2020-08-vehicle-temporal-representation.jsonld
${second_vehicle_payload_file}=     2020-09-vehicle-temporal-representation.jsonld
${bus_payload_file}=                2020-08-bus-temporal-representation.jsonld


*** Test Cases ***    PAYLOAD_FILE    EXPECTATION_FILE
021_13_01 After One Entity
    [Tags]    te-query    5_7_4
    entity-operations-after-query.jsonld    vehicles-temporal-representation-021-13-01.jsonld
021_13_02 Before One Entity
    [Tags]    te-query    5_7_4
    entity-operations-before-query.jsonld    vehicles-temporal-representation-021-13-02.jsonld
021_13_03 Between One Entity
    [Tags]    te-query    5_7_4
    entity-operations-between-query.jsonld    vehicles-temporal-representation-021-13-03.jsonld
021_13_04 After Two Entities
    [Tags]    te-query    5_7_4
    entity-operations-after-query-two-entities.jsonld    vehicles-bus-temporal-representation-021-13-04.jsonld


*** Keywords ***
Query the temporal evolution of entities using the entityOperations method
    [Documentation]    Check that one can query the temporal evolution of entities using the entityOperations method
    [Arguments]    ${payload_file}    ${expectation_file}
    ${response}=    Query Temporal Representation Of Entities Via Post
    ...    query_file_name=${payload_file}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing EntityTemporal elements
    ...    ${expectation_file}
    ...    ${response.json()}

Setup Initial Temporal Entities
    ${first_temporal_entity_representation_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-13-A
    ${create_response1}=    Create Temporal Representation Of Entity
    ...    ${first_vehicle_payload_file}
    ...    ${first_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Test Variable    ${first_temporal_entity_representation_id}
    ${second_temporal_entity_representation_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-13-B
    ${create_response2}=    Create Temporal Representation Of Entity
    ...    ${second_vehicle_payload_file}
    ...    ${second_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response2.status_code}
    Set Test Variable    ${second_temporal_entity_representation_id}
    ${third_temporal_entity_representation_id}=    Generate Bus Entity Id    021-13-A
    ${create_response3}=    Create Temporal Representation Of Entity
    ...    ${bus_payload_file}
    ...    ${third_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response3.status_code}
    Set Test Variable    ${third_temporal_entity_representation_id}

Delete Initial Temporal Entities
    Delete Temporal Representation Of Entity    ${first_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${second_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${third_temporal_entity_representation_id}
