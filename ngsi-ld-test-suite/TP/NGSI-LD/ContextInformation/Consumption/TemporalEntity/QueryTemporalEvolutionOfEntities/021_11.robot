*** Settings ***
Documentation       Check that one can query the temporal evolution of entities with a limit to the number of entities to be retrieved

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Temporal Entities
Test Teardown       Delete Initial Temporal Entities
Test Template       Query the temporal evolution of entities with a limit to the number of entities to be retrieved


*** Variables ***
${first_vehicle_payload_file}=      2020-08-vehicle-temporal-representation.jsonld
${second_vehicle_payload_file}=     2020-09-vehicle-temporal-representation.jsonld


*** Test Cases ***    LIMIT
021_11_01 Query Some Entities
    [Tags]    te-query    5_7_4
    ${2}
021_11_02 Query All Entities
    [Tags]    te-query    5_7_4
    ${3}


*** Keywords ***
Query the temporal evolution of entities with a limit to the number of entities to be retrieved
    [Documentation]    Check that one can query the temporal evolution of entities with a limit to the number of entities to be retrieved
    [Arguments]    ${limit}
    ${entity_types_to_be_retrieved}=    Catenate    SEPARATOR=,    Bus,Vehicle
    ${response}=    Query Temporal Representation Of Entities
    ...    entity_types=${entity_types_to_be_retrieved}
    ...    limit=${limit}
    ...    timerel=after
    ...    timeAt=2020-07-01T12:05:00Z
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Number Of Entities    Vehicle    ${limit}    ${response.json()}

Setup Initial Temporal Entities
    ${first_temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${create_response1}=    Create Temporal Representation Of Entity
    ...    ${first_vehicle_payload_file}
    ...    ${first_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Test Variable    ${first_temporal_entity_representation_id}
    ${second_temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${create_response2}=    Create Temporal Representation Of Entity
    ...    ${second_vehicle_payload_file}
    ...    ${second_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response2.status_code}
    Set Test Variable    ${second_temporal_entity_representation_id}
    ${third_temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${create_response3}=    Create Temporal Representation Of Entity
    ...    ${second_vehicle_payload_file}
    ...    ${third_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response3.status_code}
    Set Test Variable    ${third_temporal_entity_representation_id}

Delete Initial Temporal Entities
    Delete Temporal Representation Of Entity    ${first_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${second_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${third_temporal_entity_representation_id}
