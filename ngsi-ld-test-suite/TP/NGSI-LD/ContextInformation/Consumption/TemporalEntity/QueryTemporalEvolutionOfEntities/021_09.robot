*** Settings ***
Documentation       Check that one can query the temporal evolution of entities matching the given NGSI-LD geo-query

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Temporal Entities
Test Teardown       Delete Initial Temporal Entities
Test Template       Query the temporal evolution of entities matching the given NGSI-LD geo-query


*** Variables ***
${first_vehicle_payload_file}=      2020-08-vehicle-temporal-representation.jsonld
${second_vehicle_payload_file}=     2020-10-vehicle-temporal-representation-with-location.jsonld
${expectation_file}=                vehicles-temporal-representation-021-09.jsonld


*** Test Cases ***    GEOREL    GEOMETRY    COORDINATES    GEOPROPERTY    EXPECTATION_FILE
021_09_01 Near Point
    [Tags]    te-query    5_7_4
    near;maxDistance==2000    Point    [-8.503,41.202]    ${EMPTY}    vehicles-temporal-representation-021-09-01.jsonld
021_09_02 Within Polygon
    [Tags]    te-query    5_7_4
    within    Polygon    [[[-13.503,47.202],[6.541, 52.961],[20.37,44.653],[9.46,32.57],[-13.503,32.57],[-13.503,47.202]]]    location    vehicles-temporal-representation-021-09-02.jsonld


*** Keywords ***
Query the temporal evolution of entities matching the given NGSI-LD geo-query
    [Documentation]    Check that one can query the temporal evolution of entities matching the given NGSI-LD geo-query
    [Arguments]    ${georel}    ${geometry}    ${coordinates}    ${geoproperty}    ${expectation_file}
    ${entity_types_to_be_retrieved}=    Catenate    SEPARATOR=,    Vehicle

    ${response}=    Query Temporal Representation Of Entities
    ...    entity_types=${entity_types_to_be_retrieved}
    ...    georel=${georel}
    ...    geometry=${geometry}
    ...    coordinates=${coordinates}
    ...    geoproperty=${geoproperty}
    ...    timerel=after
    ...    timeAt=2020-07-01T12:05:00Z
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing EntityTemporal elements
    ...    ${expectation_file}
    ...    ${response.json()}

Setup Initial Temporal Entities
    ${first_temporal_entity_representation_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-09-A
    ${create_response1}=    Create Temporal Representation Of Entity
    ...    ${first_vehicle_payload_file}
    ...    ${first_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Test Variable    ${first_temporal_entity_representation_id}
    ${second_temporal_entity_representation_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-09-B
    ${create_response2}=    Create Temporal Representation Of Entity
    ...    ${second_vehicle_payload_file}
    ...    ${second_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response2.status_code}
    Set Test Variable    ${second_temporal_entity_representation_id}

Delete Initial Temporal Entities
    Delete Temporal Representation Of Entity    ${first_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${second_temporal_entity_representation_id}
