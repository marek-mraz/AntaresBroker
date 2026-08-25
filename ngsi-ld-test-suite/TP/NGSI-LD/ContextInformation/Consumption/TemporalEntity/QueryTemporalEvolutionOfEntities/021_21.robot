*** Settings ***
Documentation       Check that one can query the temporal evolution of entities with pick and omit query params

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entities
Test Teardown       Delete Initial Entities
Test Template       Query Temporal Evolution Of Entities With Pick Or Omit Query Params


*** Variables ***
${first_vehicle_payload_file}=      2020-08-vehicle-temporal-representation.jsonld
${second_vehicle_payload_file}=     2020-09-vehicle-temporal-representation.jsonld
${bus_payload_file}=                2020-08-bus-temporal-representation.jsonld


*** Test Cases ***    PICK    OMIT    EXPECTED_FILENAME    GROUP_BY
021_21_01 QueryWithPickOnAttributes
    [Documentation]    Check that one can query temporal entities with pick on attributes
    [Tags]    te-query    5_7_4    4_2_1    since_v1.8.1
    speed    ${EMPTY}    pick-omit/temporal-entities-pick-speed.json    None
021_21_02 QueryWithOmitOnAttributes
    [Documentation]    Check that one can query temporal entities with omit on attributes
    [Tags]    te-query    5_7_4    4_2_1    since_v1.8.1
    ${EMPTY}    speed    pick-omit/temporal-entities-omit-speed.json
021_21_03 QueryWithPickOnNonCoreMembers
    [Documentation]    Check that one can query temporal entities with pick on core members
    [Tags]    te-query    5_7_4    4_2_1    since_v1.8.1
    id    ${EMPTY}    pick-omit/temporal-entities-pick-id.json
021_21_04 QueryWithOmitOnNonCoreMembers
    [Documentation]    Check that one can query temporal entities with omit on core members
    [Tags]    te-query    5_7_4    4_2_1    since_v1.8.1
    ${EMPTY}    type    pick-omit/temporal-entities-omit-type.json
021_21_05 QueryWithPickOnAttributesAndCoreMembers
    [Documentation]    Check that one can query temporal entities with pick on attributes and core members
    [Tags]    te-query    5_7_4    4_2_1    since_v1.8.1
    id,speed    ${EMPTY}    pick-omit/temporal-entities-pick-id-speed.json
021_21_06 QueryWithOmitOnAttributesAndCoreMembers
    [Documentation]    Check that one can query temporal entities with omit on attributes and core members
    [Tags]    te-query    5_7_4    4_2_1    since_v1.8.1
    ${EMPTY}    type,speed    pick-omit/temporal-entities-omit-type-speed.json


*** Keywords ***
Query Temporal Evolution Of Entities With Pick Or Omit Query Params
    [Documentation]    Query the temporal evolution of entities giving pick or omit query params different values
    [Arguments]    ${pick}    ${omit}    ${expected_filename}    ${group_by}=id

    ${response}=    Query Temporal Representation Of Entities
    ...    entity_types=Bus,Vehicle
    ...    timerel=after
    ...    timeAt=1970-01-01T00:00:00Z
    ...    pick=${pick}
    ...    omit=${omit}
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing EntityTemporal elements
    ...    ${expected_filename}
    ...    ${response.json()}
    ...    ${group_by}

Setup Initial Entities
    ${first_temporal_entity_representation_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-21-A
    ${create_response1}=    Create Temporal Representation Of Entity
    ...    ${first_vehicle_payload_file}
    ...    ${first_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response1.status_code}
    Set Test Variable    ${first_temporal_entity_representation_id}
    ${second_temporal_entity_representation_id}=    Catenate    ${VEHICLE_ID_PREFIX}021-21-B
    ${create_response2}=    Create Temporal Representation Of Entity
    ...    ${second_vehicle_payload_file}
    ...    ${second_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response2.status_code}
    Set Test Variable    ${second_temporal_entity_representation_id}
    ${third_temporal_entity_representation_id}=    Catenate    ${BUS_ID_PREFIX}021-21-C
    ${create_response3}=    Create Temporal Representation Of Entity
    ...    ${bus_payload_file}
    ...    ${third_temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response3.status_code}
    Set Test Variable    ${third_temporal_entity_representation_id}

Delete Initial Entities
    Delete Temporal Representation Of Entity    ${first_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${second_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${third_temporal_entity_representation_id}
