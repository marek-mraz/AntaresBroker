*** Settings ***
Documentation       Check that one can query the temporal evolution of entities matching the given type(s)

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Temporal Entities
Suite Teardown      Delete Initial Temporal Entities


*** Variables ***
${vehicle_payload_file}=    2020-08-vehicle-temporal-representation.jsonld
${bus_payload_file}=        2020-08-bus-temporal-representation.jsonld
${expectation_file}=        vehicles-temporal-representation-021-05.jsonld


*** Test Cases ***
021_05_01 Query The Temporal Evolution Of Entities Matching The Given Type(s)
    [Documentation]    Check that one can query the temporal evolution of entities matching the given type(s)
    [Tags]    te-query    5_7_4
    ${entity_types_to_be_retrieved}=    Catenate    SEPARATOR=,    Bus

    ${response}=    Query Temporal Representation Of Entities
    ...    entity_types=${entity_types_to_be_retrieved}
    ...    timerel=after
    ...    timeAt=2020-07-01T12:05:00Z
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing EntityTemporal elements
    ...    ${expectation_file}
    ...    ${response.json()}


*** Keywords ***
Setup Initial Temporal Entities
    ${first_temporal_entity_representation_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-05-A
    ${create_response1}=    Create Temporal Representation Of Entity
    ...    ${vehicle_payload_file}
    ...    ${first_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Suite Variable    ${first_temporal_entity_representation_id}
    ${second_temporal_entity_representation_id}=    Generate Bus Entity Id    021-05-A
    ${create_response2}=    Create Temporal Representation Of Entity
    ...    ${bus_payload_file}
    ...    ${second_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response2.status_code}
    Set Suite Variable    ${second_temporal_entity_representation_id}

Delete Initial Temporal Entities
    Delete Temporal Representation Of Entity    ${first_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${second_temporal_entity_representation_id}
