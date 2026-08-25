*** Settings ***
Documentation       Check that one can query the temporal evolution of entities

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entities
Test Teardown       Delete Initial Entities
Test Template       Query the temporal evolution of entities


*** Variables ***
${first_vehicle_payload_file}=      2020-08-vehicle-temporal-representation.jsonld
${second_vehicle_payload_file}=     2020-09-vehicle-temporal-representation.jsonld
${bus_payload_file}=                2020-08-bus-temporal-representation.jsonld


*** Test Cases ***    TIMEREL    TIMEAT    ENDTIMEAT    EXPECTATION_FILE
021_01_01 After
    [Tags]    te-query    5_7_4
    after    2020-08-01T12:04:00Z    ${EMPTY}    vehicles-temporal-representation-021-01-01.jsonld
021_01_02 Before
    [Tags]    te-query    5_7_4
    before    2020-09-01T13:06:00Z    ${EMPTY}    vehicles-temporal-representation-021-01-02.jsonld
021_01_03 Between
    [Tags]    te-query    5_7_4
    between    2020-08-01T12:04:00Z    2020-09-01T13:06:00Z    vehicles-temporal-representation-021-01-03.jsonld


*** Keywords ***
Query the temporal evolution of entities
    [Documentation]    Check that one can query the temporal evolution of entities
    [Arguments]    ${timerel}    ${timeAt}    ${endTimeAt}    ${expectation_file}
    ${entity_types_to_be_retrieved}=    Catenate    SEPARATOR=,    Vehicle

    ${response}=    Query Temporal Representation Of Entities
    ...    entity_types=${entity_types_to_be_retrieved}
    ...    timerel=${timerel}
    ...    timeAt=${timeAt}
    ...    endTimeAt=${endTimeAt}
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing EntityTemporal elements
    ...    ${expectation_file}
    ...    ${response.json()}

Setup Initial Entities
    ${first_temporal_entity_representation_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-01-A
    ${create_response1}=    Create Temporal Representation Of Entity
    ...    ${first_vehicle_payload_file}
    ...    ${first_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Test Variable    ${first_temporal_entity_representation_id}
    ${second_temporal_entity_representation_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-01-B
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

Delete Initial Entities
    Delete Temporal Representation Of Entity    ${first_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${second_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${third_temporal_entity_representation_id}
