*** Settings ***
Documentation       Check that one can query all temporal evolution of entities if query is local

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Temporal Entities
Suite Teardown      Delete Initial Temporal Entities


*** Variables ***
${first_vehicle_payload_file}=      2020-08-vehicle-temporal-representation.jsonld
${second_vehicle_payload_file}=     2020-09-vehicle-temporal-representation.jsonld
# 021_25 creates BOTH 021-06-A and 021-06-B; local=true (NGSI-LD 6.3.18 / 5.5.13) only excludes
# Context Source Registrations, so both LOCAL entities are returned. The shared 021-06 expectation
# lists only A (correct for 021_06, which creates only A); use a 021_25-specific one with A and B.
${expectation_file}=                vehicles-temporal-representation-021-25.jsonld


*** Test Cases ***
021_25 Query All Temporal Entities With Local Flag
    [Documentation]    Check that one can query all temporal evolution of entities if query is local
    [Tags]    te-query    5_7_4    not-implemented

    ${response}=    Query Temporal Representation Of Entities
    ...    timerel=after
    ...    timeAt=2020-07-01T12:05:00Z
    ...    local=true
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing EntityTemporal elements
    ...    ${expectation_file}
    ...    ${response.json()}


*** Keywords ***
Setup Initial Temporal Entities
    ${first_temporal_entity_representation_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-06-A
    ${create_response1}=    Create Temporal Representation Of Entity
    ...    ${first_vehicle_payload_file}
    ...    ${first_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Suite Variable    ${first_temporal_entity_representation_id}
    ${second_temporal_entity_representation_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-06-B
    ${create_response2}=    Create Temporal Representation Of Entity
    ...    ${second_vehicle_payload_file}
    ...    ${second_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response2.status_code}
    Set Suite Variable    ${second_temporal_entity_representation_id}

Delete Initial Temporal Entities
    Delete Temporal Representation Of Entity    ${first_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${second_temporal_entity_representation_id}
