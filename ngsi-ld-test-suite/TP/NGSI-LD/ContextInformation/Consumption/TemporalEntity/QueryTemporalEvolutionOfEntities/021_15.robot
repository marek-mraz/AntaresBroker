*** Settings ***
Documentation       Check temporal pagination is applied when querying the temporal evolution of entities

Library             DateTime
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Initial Entities
Test Template       Retrieve Temporal Entities


*** Variables ***
${first_vehicle_payload_file}=      pagination/2020-01-vehicule-temporal-representation-twenty-instances.jsonld
${second_vehicle_payload_file}=     2020-09-vehicle-temporal-representation.jsonld
${timeBefore}=                      2019-01-01T01:01:00Z
${timeAfter}=                       2021-01-01T01:01:00Z
${lrt}=                             least recent timestamp
${mrt}=                             most recent timestamp


*** Test Cases ***
021_15_01 Retrieve The Entity With lastN And Timerel Before
    [Tags]    te-retrieve    5_7_4    6_3_10    since_v1.5.1
    lastN=${20}    expectedSize=20    timerel=before    timeAt=${timeAfter}    expectedRangeStart=${timeAfter}    expectedRangeEnd=${lrt}
021_15_02 Retrieve The Entity With lastN And Timerel Between
    [Tags]    te-retrieve    5_7_4    6_3_10    since_v1.5.1
    lastN=${20}    timerel=between    timeAt=${timeBefore}    endTimeAt=${timeAfter}    expectedRangeStart=${timeAfter}    expectedRangeEnd=${lrt}
021_15_03 Retrieve The Entity With lastN And Timerel After
    [Tags]    te-retrieve    5_7_4    6_3_10    since_v1.5.1
    lastN=${20}    expectedSize=20    timerel=after    timeAt=${timeBefore}    expectedRangeStart=${mrt}    expectedRangeEnd=${lrt}
021_15_04 Retrieve The Entity With Timerel Before
    [Tags]    te-retrieve    5_7_4    6_3_10    since_v1.5.1
    timerel=before    expectedSize=*    timeAt=${timeAfter}    expectedRangeEnd=${mrt}
021_15_05 Retrieve The Entity With Timerel Between
    [Tags]    te-retrieve    5_7_4    6_3_10    since_v1.5.1
    timerel=between    timeAt=${timeBefore}    endTimeAt=${timeAfter}    expectedRangeStart=${timeBefore}    expectedRangeEnd=${mrt}
021_15_06 Retrieve The Entity With Timerel After
    [Tags]    te-retrieve    5_7_4    6_3_10    since_v1.5.1
    timerel=after    timeAt=${timeBefore}    expectedRangeStart=${timeBefore}    expectedRangeEnd=${mrt}
021_15_07 Retrieve The Entity With temporalValues And Timerel After
    [Tags]    te-retrieve    5_7_4    6_3_10    since_v1.5.1
    representation=temporalValues    timerel=after    timeAt=${timeBefore}    expectedRangeStart=${timeBefore}    expectedRangeEnd=${mrt}
021_15_08 Retrieve The Entity With temporalValues, lastN And Timerel Between
    [Tags]    te-retrieve    5_7_4    6_3_10    since_v1.5.1
    representation=temporalValues    lastN=${20}    timerel=between    timeAt=${timeBefore}    endTimeAt=${timeAfter}    expectedRangeStart=${timeAfter}    expectedRangeEnd=${lrt}


*** Keywords ***
Retrieve Temporal Entities
    [Documentation]    Check temporal pagination behavior on multiple entity endpoint
    [Arguments]
    ...    ${representation}=${EMPTY}
    ...    ${timerel}=${EMPTY}
    ...    ${timeAt}=${EMPTY}
    ...    ${endTimeAt}=${EMPTY}
    ...    ${lastN}=${EMPTY}
    ...    ${expectedRangeStart}=${EMPTY}
    ...    ${expectedRangeEnd}=${EMPTY}
    ...    ${expectedSize}=${EMPTY}
    ${entity_types_to_be_retrieved}=    Catenate    SEPARATOR=,    Vehicle

    ${response}=    Query Temporal Representation Of Entities
    ...    entity_types=${entity_types_to_be_retrieved}
    ...    options=${representation}
    ...    context=${ngsild_test_suite_context}
    ...    timerel=${timerel}
    ...    timeAt=${timeAt}
    ...    endTimeAt=${endTimeAt}
    ...    lastN=${lastN}
    Check Response Status Code    206    ${response.status_code}

    ${contentRange}=    Get Regexp Matches
    ...    ${response.headers}[Content-Range]
    ...    ([a-zA-Z\-]+) (.*-.*-.*)-(.*-.*-.*)\/(.*)
    ...    1
    ...    2
    ...    3
    ...    4
    ${unit}=    Set Variable    ${contentRange}[0][0]
    ${rangeStart}=    Convert Date    ${contentRange}[0][1]
    ${rangeEnd}=    Convert Date    ${contentRange}[0][2]
    ${size}=    Set Variable    ${contentRange}[0][3]

    Check Content Range Part Equal    ${unit}    date-time

    IF    $expectedRangeStart != ''
        IF    $expectedRangeStart == $lrt
            ${expectedRangeStart}=    Get Least Recent Timestamp From Vehicles    ${response.json()}
        ELSE IF    $expectedRangeStart == $mrt
            ${expectedRangeStart}=    Get Most Recent Timestamp From Vehicles    ${response.json()}
        ELSE
            ${expectedRangeStart}=    Convert Date    ${expectedRangeStart}
        END
        Check Content Range Part Equal    ${expectedRangeStart}    ${rangeStart}
    END

    IF    $expectedRangeEnd != ''
        IF    $expectedRangeEnd == $lrt
            ${expectedRangeEnd}=    Get Least Recent Timestamp From Vehicles
            ...    ${response.json()}
            ...    ${representation}
        ELSE IF    $expectedRangeEnd == $mrt
            ${expectedRangeEnd}=    Get Most Recent Timestamp From Vehicles
            ...    ${response.json()}
            ...    ${representation}
        ELSE
            ${expectedRangeEnd}=    Convert Date    ${expectedRangeEnd}
        END
        Check Content Range Part Equal    ${expectedRangeEnd}    ${rangeEnd}
    END

    IF    $expectedSize != ''
        Check Content Range Part Equal    ${expectedSize}    ${size}
    END

Setup Initial Entities
    ${first_temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${second_temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${response}=    Create Temporal Representation Of Entity
    ...    ${first_vehicle_payload_file}
    ...    ${first_temporal_entity_representation_id}
    Check Response Status Code    201    ${response.status_code}

    ${response}=    Create Temporal Representation Of Entity
    ...    ${second_vehicle_payload_file}
    ...    ${second_temporal_entity_representation_id}
    Check Response Status Code    201    ${response.status_code}

    Set Suite Variable    ${first_temporal_entity_representation_id}
    Set Suite Variable    ${second_temporal_entity_representation_id}

Delete Initial Entities
    Delete Temporal Representation Of Entity    ${first_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${second_temporal_entity_representation_id}

Get Least Recent Timestamp From Vehicles
    [Arguments]    ${body}    ${representation}=${EMPTY}

    ${attributeList}=    Get Attribute List From Vehicle Body    ${body}    ${representation}

    ${leastRecentTimestamp}=    Convert Date    ${timeAfter}
    FOR    ${attribute}    IN    @{attributeList}
        IF    $representation == 'temporalValues'
            ${attributeTime}=    Convert Date    ${attribute}[1]
        ELSE
            ${attributeTime}=    Convert Date    ${attribute}[observedAt]
        END

        IF    $leastRecentTimestamp >= $attributeTime
            ${leastRecentTimestamp}=    Set Variable    ${attributeTime}
        END
    END
    RETURN    ${leastRecentTimestamp}

Get Most Recent Timestamp From Vehicles
    [Arguments]    ${body}    ${representation}=${EMPTY}
    ${attributeList}=    Get Attribute List From Vehicle Body    ${body}    ${representation}

    ${mostRecentTimestamp}=    Convert Date    ${timeBefore}
    FOR    ${attribute}    IN    @{attributeList}
        IF    $representation == 'temporalValues'
            ${attributeTime}=    Convert Date    ${attribute}[1]
        ELSE
            ${attributeTime}=    Convert Date    ${attribute}[observedAt]
        END
        IF    $mostRecentTimestamp <= $attributeTime
            ${mostRecentTimestamp}=    Set Variable    ${attributeTime}
        END
    END
    RETURN    ${mostRecentTimestamp}

Get Attribute List From Vehicle Body
    [Arguments]    ${body}    ${representation}=${EMPTY}
    IF    $representation == 'temporalValues'
        ${attributeList}=    Combine Lists
        ...    ${body}[0][speed][values]
        ...    ${body}[0][fuelLevel][values]
        ...    ${body}[1][speed][values]
        ...    ${body}[1][fuelLevel][values]
    ELSE
        ${attributeList}=    Combine Lists
        ...    ${body}[0][speed]
        ...    ${body}[0][fuelLevel]
        ...    ${body}[1][speed]
        ...    ${body}[1][fuelLevel]
    END
    RETURN    ${attributeList}
