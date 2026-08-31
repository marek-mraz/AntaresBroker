*** Settings ***
Documentation       Verify the 4.15 language filter on the temporal forms
...                 (Table 6.18.3.2-1 and 6.19.3.1 `lang`): a
...                 LanguageProperty instance "shall be converted into a
...                 Property" holding the languageMap entry of the chosen
...                 language, with the non-reified `lang` member naming it;
...                 with no match "a single language shall be chosen, up
...                 to the implementation"; in the temporalValues form the
...                 reduced Property renders as 4.5.9 `values` pairs.
...                 Antares extension TP.

Library             RequestsLibrary
Library             Collections
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Create Fixture Entity
Suite Teardown      Delete Fixture Entity


*** Variables ***
${eid}=         urn:ngsi-ld:Vehicle:lang5744
${window}=      timerel=after&timeAt=2026-03-01T00:00:00Z


*** Test Cases ***
5744_07_01 Lang Reduces Every Instance On The Temporal Query
    [Documentation]    Table 6.18.3.2-1 `lang`: both instances of the
    ...    LanguageProperty come back as Property/value/lang in French,
    ...    the plain Property is untouched.
    [Tags]    te-query    5_7_4    4_15    since_v1.9.1
    ${response}=    Query Temporal    lang=fr
    ${instances}=    Label Instances    ${response.json()[0]}
    Length Should Be    ${instances}    2
    FOR    ${inst}    IN    @{instances}
        Should Be Equal    ${inst}[type]    Property
        Should Be Equal    ${inst}[lang]    fr
        Should Be True    $inst['value'] in ('bonjour', 'salut')
        Dictionary Should Not Contain Key    ${inst}    languageMap
    END
    Should Not Contain    ${response.text}    languageMap
    Should Contain    ${response.text}    "value":30

5744_07_02 Lang Reduces On The Temporal Retrieve
    [Documentation]    Table 6.19.3.1 `lang`: the same reduction on
    ...    GET /temporal/entities/{id}.
    [Tags]    te-retrieve    5_7_3    4_15    since_v1.9.1
    ${response}=    GET    url=${temporal_api_url}/temporal/entities/${eid}
    ...    params=lang=en&${window}    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    ${instances}=    Label Instances    ${response.json()}
    Length Should Be    ${instances}    2
    FOR    ${inst}    IN    @{instances}
        Should Be Equal    ${inst}[type]    Property
        Should Be Equal    ${inst}[lang]    en
        Should Be True    $inst['value'] in ('hello', 'bye')
    END
    Should Not Contain    ${response.text}    languageMap

5744_07_03 Lang With TemporalValues Renders Values Pairs
    [Documentation]    4.5.9 on the reduced Property: `values` pairs of the
    ...    chosen string and the timestamp, never `languageMaps`.
    [Tags]    te-query    5_7_4    4_5_9    since_v1.9.1
    ${response}=    Query Temporal    lang=fr&format=temporalValues
    ${label}=    Set Variable    ${response.json()[0]}[label]
    Should Be Equal    ${label}[type]    Property
    Dictionary Should Not Contain Key    ${label}    languageMaps
    ${strings}=    Evaluate    sorted(p[0] for p in $label['values'])
    Should Be Equal    ${strings}    ${{ ['bonjour', 'salut'] }}

5744_07_04 Without Lang The LanguageMap Is Kept
    [Documentation]    No language filter: the LanguageProperty keeps its
    ...    type and full languageMap, no `lang` member appears.
    [Tags]    te-query    5_7_4    4_5_18    since_v1.9.1
    ${response}=    Query Temporal    ${EMPTY}
    ${instances}=    Label Instances    ${response.json()[0]}
    FOR    ${inst}    IN    @{instances}
        Should Be Equal    ${inst}[type]    LanguageProperty
        Dictionary Should Contain Key    ${inst}[languageMap]    en
        Dictionary Should Contain Key    ${inst}[languageMap]    fr
        Dictionary Should Not Contain Key    ${inst}    lang
        Dictionary Should Not Contain Key    ${inst}    value
    END

5744_07_05 Lang Without A Match Falls Back To One Language
    [Documentation]    5.7.2.5 wording, bound to 6.18.3.2-1: with no
    ...    German entry a single available language is chosen and named.
    [Tags]    te-query    5_7_4    4_15    since_v1.9.1
    ${response}=    Query Temporal    lang=de
    ${instances}=    Label Instances    ${response.json()[0]}
    FOR    ${inst}    IN    @{instances}
        Should Be Equal    ${inst}[type]    Property
        Should Be True    $inst['lang'] in ('en', 'fr')
        Should Be True    $inst['value'] in ('hello', 'bye', 'bonjour', 'salut')
        Dictionary Should Not Contain Key    ${inst}    languageMap
    END


*** Keywords ***
Query Temporal
    [Arguments]    ${extra}
    ${response}=    GET
    ...    url=${temporal_api_url}/temporal/entities
    ...    params=type=Vehicle&id=${eid}&${window}&${extra}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    RETURN    ${response}

Label Instances
    [Documentation]    5.2.20: a single instance may be rendered bare, a
    ...    series as an array — normalize to a list.
    [Arguments]    ${entity}
    ${label}=    Set Variable    ${entity}[label]
    ${instances}=    Evaluate    $label if isinstance($label, list) else [$label]
    RETURN    ${instances}

Create Fixture Entity
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${temporal_api_url}/temporal/entities
    ...    data={"id": "${eid}", "type": "Vehicle", "label": [{"type": "LanguageProperty", "languageMap": {"en": "hello", "fr": "bonjour"}, "observedAt": "2026-03-01T12:00:00Z"}, {"type": "LanguageProperty", "languageMap": {"en": "bye", "fr": "salut"}, "observedAt": "2026-03-01T13:00:00Z"}], "speed": [{"type": "Property", "value": 30, "observedAt": "2026-03-01T12:00:00Z"}]}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}

Delete Fixture Entity
    ${response}=    DELETE    url=${temporal_api_url}/temporal/entities/${eid}    expected_status=any
