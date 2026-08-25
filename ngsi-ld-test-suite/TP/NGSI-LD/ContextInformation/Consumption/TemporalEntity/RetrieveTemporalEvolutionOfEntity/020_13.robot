*** Settings ***
Documentation       Check temporal pagination is applied when retrieving the temporal evolution of an entity

Library             DateTime
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Temporal Entity
Suite Teardown      Delete Initial Temporal Entity
Test Template       Retrieve Temporal Entity


*** Variables ***
${vehicle_payload_file}=    pagination/2020-01-vehicule-temporal-representation-twenty-instances.jsonld
${timeBefore}=              2019-01-01T01:01:00Z
${timeAfter}=               2021-01-01T01:01:00Z
${lrt}=                     least recent timestamp
${mrt}=                     most recent timestamp


*** Test Cases ***
020_13_01 Retrieve An Entity With 20 Instances On Two Attributes
    [Tags]    te-retrieve    5_7_3    6_3_10    since_v1.5.1
    ${EMPTY}    expectedSize=*    expectedRangeStart=${lrt}    expectedRangeEnd=${mrt}
020_13_02 Retrieve The Entity With lastN
    [Tags]    te-retrieve    5_7_3    6_3_10    since_v1.5.1
    lastN=${20}    expectedSize=20    expectedRangeStart=${mrt}    expectedRangeEnd=${lrt}
020_13_03 Retrieve The Entity With lastN And Timerel Before
    [Tags]    te-retrieve    5_7_3    6_3_10    since_v1.5.1
    lastN=${20}    expectedSize=20    timerel=before    timeAt=${timeAfter}    expectedRangeStart=${timeAfter}    expectedRangeEnd=${lrt}
020_13_04 Retrieve The Entity With lastN And Timerel Between
    [Tags]    te-retrieve    5_7_3    6_3_10    since_v1.5.1
    lastN=${20}    timerel=between    timeAt=${timeBefore}    endTimeAt=${timeAfter}    expectedRangeStart=${timeAfter}    expectedRangeEnd=${lrt}
020_13_05 Retrieve The Entity With lastN And Timerel After
    [Tags]    te-retrieve    5_7_3    6_3_10    since_v1.5.1
    lastN=${20}    expectedSize=20    timerel=after    timeAt=${timeBefore}    expectedRangeStart=${mrt}    expectedRangeEnd=${lrt}
020_13_06 Retrieve The Entity With Timerel Before
    [Tags]    te-retrieve    5_7_3    6_3_10    since_v1.5.1
    timerel=before    expectedSize=*    timeAt=${timeAfter}    expectedRangeEnd=${mrt}
020_13_07 Retrieve The Entity With Timerel Between
    [Tags]    te-retrieve    5_7_3    6_3_10    since_v1.5.1
    timerel=between    timeAt=${timeBefore}    endTimeAt=${timeAfter}    expectedRangeStart=${timeBefore}    expectedRangeEnd=${mrt}
020_13_08 Retrieve The Entity With Timerel After
    [Tags]    te-retrieve    5_7_3    6_3_10    since_v1.5.1
    timerel=after    timeAt=${timeBefore}    expectedRangeStart=${timeBefore}    expectedRangeEnd=${mrt}
020_13_09 Retrieve The Entity With temporalValues And Timerel After
    [Tags]    te-retrieve    5_7_3    6_3_10    since_v1.5.1
    representation=temporalValues    timerel=after    timeAt=${timeBefore}    expectedRangeStart=${timeBefore}    expectedRangeEnd=${mrt}
020_13_10 Retrieve The Entity With temporalValues, lastN And Timerel Between
    [Tags]    te-retrieve    5_7_3    6_3_10    since_v1.5.1
    representation=temporalValues    lastN=${20}    timerel=between    timeAt=${timeBefore}    endTimeAt=${timeAfter}    expectedRangeStart=${timeAfter}    expectedRangeEnd=${lrt}


*** Keywords ***
Retrieve Temporal Entity
    [Documentation]    Check temporal pagination behavior
    [Arguments]
    ...    ${representation}=${EMPTY}
    ...    ${timerel}=${EMPTY}
    ...    ${timeAt}=${EMPTY}
    ...    ${endTimeAt}=${EMPTY}
    ...    ${lastN}=${EMPTY}
    ...    ${expectedRangeStart}=${EMPTY}
    ...    ${expectedRangeEnd}=${EMPTY}
    ...    ${expectedSize}=${EMPTY}
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
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
            ${expectedRangeStart}=    Get Least Recent Timestamp From Vehicle Body    ${response.json()}
        ELSE IF    $expectedRangeStart == $mrt
            ${expectedRangeStart}=    Get Most Recent Timestamp From Vehicle Body    ${response.json()}
        ELSE
            ${expectedRangeStart}=    Convert Date    ${expectedRangeStart}
        END
        Check Content Range Part Equal    ${expectedRangeStart}    ${rangeStart}
    END

    IF    $expectedRangeEnd != ''
        IF    $expectedRangeEnd == $lrt
            ${expectedRangeEnd}=    Get Least Recent Timestamp From Vehicle Body
            ...    ${response.json()}
            ...    ${representation}
        ELSE IF    $expectedRangeEnd == $mrt
            ${expectedRangeEnd}=    Get Most Recent Timestamp From Vehicle Body
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

Create Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${response}=    Create Temporal Representation Of Entity
    ...    ${vehicle_payload_file}
    ...    ${temporal_entity_representation_id}
    Check Response Status Code    201    ${response.status_code}

    Set Suite Variable    ${temporal_entity_representation_id}

Delete Initial Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}

Get Least Recent Timestamp From Vehicle Body
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

Get Most Recent Timestamp From Vehicle Body
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
        ${attributeList}=    Combine Lists    ${body}[speed][values]    ${body}[fuelLevel][values]
    ELSE
        ${attributeList}=    Combine Lists    ${body}[speed]    ${body}[fuelLevel]
    END
    RETURN    ${attributeList}
